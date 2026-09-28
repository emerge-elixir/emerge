defmodule Emerge.Benchmarks.Support do
  @moduledoc false
  @root Path.expand("../..", __DIR__)
  @source_paths ~w(native/emerge_skia lib test mix priv/test_assets priv/sample_assets config .cargo mix.exs
    mix.lock rust-toolchain.toml ci-tests.sh scripts/performance-lock.sh scripts/benchmarks bench)
  @environment ~w(RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_TARGET_DIR CC CXX MALLOC_ARENA_MAX
    LD_PRELOAD SKIA_BINARIES_URL EMERGE_PERFORMANCE_LOCK)

  def prepare!(output, scope) do
    File.cd!(@root)
    [_, parent] = Regex.run(~r/^PPid:\s+(\d+)$/m, File.read!("/proc/self/status"))

    unless exclusive_parent?(File.read!("/proc/#{parent}/cmdline")) do
      raise "Run directly through scripts/performance-lock.sh exclusive"
    end

    output = Path.expand(output)
    File.mkdir_p!(Path.dirname(output))
    File.mkdir!(output)
    paths = sources()
    manifest = manifest(paths)
    identity = "source-sha256:" <> digest(manifest)
    System.put_env("EMERGE_SOURCE_REVISION", identity)
    File.write!(Path.join(output, "source.sha256"), manifest)
    listing = Path.join(output, "source-files.txt")
    File.write!(listing, [Enum.intersperse(paths, <<0>>), <<0>>])

    capture!("tar", [
      "--create",
      "--gzip",
      "--dereference",
      "--file",
      Path.join(output, "source.tar.gz"),
      "--null",
      "--files-from",
      listing
    ])

    write_json!(output, "identity.json", %{
      source: identity,
      head_for_reference_only: capture!("git", ["rev-parse", "HEAD"]),
      rustc: capture!("rustc", ["-Vv"]),
      cargo: capture!("cargo", ["-V"]),
      host: capture!("uname", ["-a"]),
      cpu: capture!("lscpu", []),
      affinity: affinity(),
      features: "default + bench-diagnostics",
      environment: Map.new(@environment, &{&1, System.get_env(&1)}),
      scope: scope
    })

    File.write!(
      Path.join(output, "worktree-status.txt"),
      capture!("git", ["status", "--short"]) <> "\n"
    )

    %{output: output, paths: paths, manifest: manifest, identity: identity}
  end

  def exclusive_parent?(command) do
    case String.split(command, <<0>>, trim: true) do
      [executable | args] -> Path.basename(executable) == "flock" and "-x" in args
      [] -> false
    end
  end

  def build!(snapshot, args, target) do
    write_json!(snapshot.output, "command.json", ["cargo" | args])
    stderr = Path.join(snapshot.output, "build.log")

    {_, status} =
      System.cmd(
        "bash",
        ["-c", "exec \"$@\" 2> \"$EMERGE_BENCH_STDERR\"", "benchmark", "cargo" | args],
        env: [{"EMERGE_BENCH_STDERR", stderr}],
        into: File.stream!(Path.join(snapshot.output, "build.jsonl"))
      )

    check_status!(status, "cargo")
    binary = snapshot.output |> Path.join("build.jsonl") |> File.read!() |> artifact!(target)
    directory = capture!("mktemp", ["-d", "-t", "emerge-benchmark-XXXXXXXX"])
    copy = Path.join(directory, Path.basename(binary))
    File.cp!(binary, copy)
    File.chmod!(copy, Bitwise.band(File.stat!(binary).mode, 0o777))
    digest = copy |> File.read!() |> digest()
    File.write!(Path.join(snapshot.output, "binary.sha256"), "#{digest}  #{copy}\n")
    verify!(snapshot)
    Map.merge(snapshot, %{binary: copy, binary_digest: digest})
  end

  def artifact!(jsonl, target) do
    artifact =
      jsonl
      |> String.split("\n", trim: true)
      |> Enum.map(&JSON.decode!/1)
      |> Enum.find(fn item ->
        item["reason"] == "compiler-artifact" and is_binary(item["executable"]) and
          get_in(item, ["target", "name"]) == target
      end)

    if artifact, do: artifact["executable"], else: raise("No executable artifact for #{target}")
  end

  def run_logged!(executable, args, log, env \\ []) do
    {_, status} =
      System.cmd(executable, args,
        env: env,
        stderr_to_stdout: true,
        into: File.stream!(log, [:append])
      )

    check_status!(status, executable)
  end

  def verify!(snapshot) do
    unless sources() == snapshot.paths and manifest(snapshot.paths) == snapshot.manifest do
      raise "Source changed during build or measurement"
    end

    if Map.has_key?(snapshot, :binary) and
         digest(File.read!(snapshot.binary)) != snapshot.binary_digest do
      raise "Binary changed during measurement"
    end
  end

  def write_json!(output, name, value) do
    File.write!(Path.join(output, name), [JSON.encode!(value), "\n"])
  end

  def rotate(values, offset) do
    {first, rest} = Enum.split(values, offset)
    Enum.concat(rest, first)
  end

  def median(values) do
    count = length(values)
    {lower, [middle | _]} = values |> Enum.sort() |> Enum.split(div(count, 2))
    if rem(count, 2) == 1, do: middle, else: (List.last(lower) + middle) / 2
  end

  def decimal(value, precision), do: :erlang.float_to_binary(value / 1, decimals: precision)
  def digest(bytes), do: :crypto.hash(:sha256, bytes) |> Base.encode16(case: :lower)

  defp sources do
    capture!("git", [
      "ls-files",
      "--cached",
      "--others",
      "--exclude-standard",
      "-z",
      "--" | @source_paths
    ])
    |> String.split(<<0>>, trim: true)
    |> Enum.filter(&File.regular?/1)
    |> Enum.uniq()
    |> Enum.sort()
  end

  defp manifest(paths), do: Enum.map_join(paths, &"#{digest(File.read!(&1))}  #{&1}\n")

  defp affinity do
    [_, cpus] = Regex.run(~r/^Cpus_allowed_list:\s+([^\n]+)$/m, File.read!("/proc/self/status"))

    cpus
    |> String.split(",")
    |> Enum.flat_map(fn segment ->
      case String.split(segment, "-") do
        [cpu] -> [String.to_integer(cpu)]
        [first, last] -> Enum.to_list(String.to_integer(first)..String.to_integer(last))
      end
    end)
    |> Enum.sort()
  end

  defp capture!(executable, args) do
    {output, status} = System.cmd(executable, args)
    check_status!(status, executable)
    String.trim_trailing(output, "\n")
  end

  defp check_status!(0, _executable), do: :ok
  defp check_status!(status, executable), do: raise("#{executable} exited with status #{status}")
end
