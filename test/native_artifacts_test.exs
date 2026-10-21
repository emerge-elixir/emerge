Code.require_file("../mix/native_artifacts.exs", __DIR__)

defmodule Emerge.NativeArtifactsTest do
  use ExUnit.Case, async: true
  import ExUnit.CaptureIO

  alias Emerge.Mix.NativeArtifacts
  alias EmergeSkia.BuildProfiles

  setup do
    root =
      Path.join(System.tmp_dir!(), "emerge-native-build-#{System.unique_integer([:positive])}")

    File.mkdir_p!(root)
    on_exit(fn -> File.rm_rf!(root) end)
    %{root: root, output: Path.join(root, "artifacts")}
  end

  test "six architecture/platform builders preserve all 48 archives" do
    builders = BuildProfiles.release_builders()

    assert Map.new(builders, &{&1.name, &1.artifact_count}) == %{
             "linux_aarch64" => 16,
             "linux_armv7" => 4,
             "linux_riscv64gc" => 4,
             "linux_x86_64" => 20,
             "macos_aarch64" => 2,
             "macos_x86_64" => 2
           }

    assert builders |> Enum.flat_map(& &1.targets) |> Enum.sort() ==
             Enum.sort(BuildProfiles.targets())

    assert builders |> Enum.flat_map(&NativeArtifacts.jobs/1) |> Enum.sort() ==
             Enum.sort(BuildProfiles.release_jobs())

    x86 = Enum.find(builders, &(&1.name == "linux_x86_64"))
    assert x86.targets == ["x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl"]
    assert x86.system_packages =~ "libwayland-dev"
    assert x86.system_packages |> String.split() |> Enum.uniq() |> length() == 7
  end

  test "AArch64 profiles share Cargo output and snapshot every exact build", context do
    build!("linux_aarch64", context)
    jobs = Enum.filter(BuildProfiles.release_jobs(), &(&1.target == "aarch64-unknown-linux-gnu"))
    assert length(File.ls!(context.output)) == 16

    for job <- jobs do
      archive = Path.join(context.output, NativeArtifacts.archive_name(job, "0.5.1"))
      assert [{name, bytes}] = archive_entries(archive)
      assert to_string(name) == String.replace_suffix(Path.basename(archive), ".tar.gz", "")
      assert bytes == job.cargo_features
    end

    builds = commands() |> Enum.filter(&match?({"cargo", ["build" | _], _}, &1))
    assert length(builds) == 16

    assert builds
           |> Enum.map(fn {_, _, opts} -> Map.new(opts[:env])["CARGO_TARGET_DIR"] end)
           |> Enum.uniq() ==
             [Path.join(context.root, "native/emerge_skia/target")]

    for {_, args, opts} <- builds do
      features = value(args, "--features")
      job = Enum.find(jobs, &(&1.cargo_features == features))
      env = Map.new(opts[:env])
      assert env["FORCE_SKIA_BUILD"] == if(job.embedded_skia, do: "1")
      assert env["SKIA_GN_ARGS"] != nil == job.embedded_skia
      assert env["RUSTFLAGS"] != nil == job.embedded_skia
    end

    fingerprints =
      Enum.map(builds, fn {_, args, opts} ->
        apis =
          String.split(value(args, "--features"), ",") |> Enum.filter(&(&1 in ~w(opengl vulkan)))

        {Map.new(opts[:env])["SKIA_GN_ARGS"], apis}
      end)

    batches = Enum.chunk_by(fingerprints, & &1) |> Enum.map(&hd/1)
    assert length(batches) == length(Enum.uniq(fingerprints))
  end

  test "macOS raster is packaged before the same builder compiles its Metal host", context do
    build!("macos_aarch64", context)

    nif =
      Path.join(context.output, "libemerge_skia-v0.5.1-nif-2.15-aarch64-apple-darwin.so.tar.gz")

    host = Path.join(context.output, "macos_host-v0.5.1-aarch64-apple-darwin.tar.gz")
    assert [{_, "embedded-cpu"}] = archive_entries(nif)
    assert [{~c"macos_host", "macos"}] = archive_entries(host)
    checksum = :crypto.hash(:sha256, File.read!(host)) |> Base.encode16(case: :lower)
    assert File.read!(host <> ".sha256") == "#{checksum}  #{Path.basename(host)}\n"

    builds = commands() |> Enum.filter(&match?({"cargo", ["build" | _], _}, &1))
    assert [{_, _, nif_opts}, {_, host_args, host_opts}] = builds
    assert "macos_host" in host_args

    assert Map.new(nif_opts[:env])["CARGO_TARGET_DIR"] ==
             Map.new(host_opts[:env])["CARGO_TARGET_DIR"]

    assert Map.new(nif_opts[:env])["FORCE_SKIA_BUILD"] == "1"
    assert Map.new(host_opts[:env])["FORCE_SKIA_BUILD"] == nil
  end

  test "container profiles reuse one image, registry and ABI-specific build directory", context do
    build!("linux_riscv64gc", context)
    calls = commands()
    assert Enum.count(calls, &match?({"docker", ["build" | _], _}, &1)) == 1
    runs = Enum.filter(calls, &match?({"docker", ["run" | _], _}, &1))
    assert length(runs) == 4

    for {_, args, _} <- runs do
      assert "CARGO_HOME=/build/cargo" in args
      assert "type=bind,source=#{context.root},target=/source,readonly" in args

      assert "type=bind,source=#{context.root}/native-build/riscv64gc-unknown-linux-gnu,target=/build" in args

      assert Enum.any?(args, &String.starts_with?(&1, "EMERGE_CARGO_FEATURES="))
      assert Enum.any?(args, &String.starts_with?(&1, "EMERGE_VERIFY_PROFILE="))
    end

    assert length(File.ls!(context.output)) == 4
  end

  test "ARMv7 profiles share cross output and keep embedded build settings", context do
    build!("linux_armv7", context)
    builds = commands() |> Enum.filter(&match?({"cross", ["build" | _], _}, &1))
    assert length(builds) == 4
    assert length(File.ls!(context.output)) == 4

    for {_, args, opts} <- builds do
      assert value(args, "--target") == "armv7-unknown-linux-gnueabihf"
      assert Map.new(opts[:env])["FORCE_SKIA_BUILD"] == "1"

      assert Map.new(opts[:env])["CARGO_TARGET_DIR"] ==
               Path.join(context.root, "native/emerge_skia/target")
    end
  end

  test "GNU and musl share a builder but not ABI build directories", context do
    build!("linux_x86_64", context)
    calls = commands()
    builds = Enum.filter(calls, &match?({"cargo", ["build" | _], _}, &1))
    runs = Enum.filter(calls, &match?({"docker", ["run" | _], _}, &1))
    assert length(builds) == 16
    assert length(runs) == 4
    assert length(File.ls!(context.output)) == 20

    for {_, args, _} <- builds, do: assert(value(args, "--target") == "x86_64-unknown-linux-gnu")

    for {_, args, _} <- runs do
      assert "x86_64-unknown-linux-musl" in args

      assert "type=bind,source=#{context.root}/native-build/x86_64-unknown-linux-musl,target=/build" in args
    end
  end

  test "a failed build stops its builder without packaging that profile", context do
    run = fn
      "cargo", ["build" | _] = args, opts ->
        send(self(), {:command, "cargo", args, opts})
        {"failure", 23}

      program, args, opts ->
        fake_command(program, args, opts)
    end

    capture_io(fn ->
      assert_raise RuntimeError, ~r/failed \(23\)/, fn ->
        NativeArtifacts.build!("linux_aarch64", "0.5.1",
          root: context.root,
          output: context.output,
          run: run
        )
      end
    end)

    assert File.ls!(context.output) == []
    builds = commands() |> Enum.filter(&match?({"cargo", ["build" | _], _}, &1))
    assert [{"cargo", ["build" | _], _}] = builds
  end

  test "an incompatible Rustler ABI fails before building or packaging", context do
    run = fn
      "cargo", ["tree" | _], _ -> {"nif_version_2_15,nif_version_2_16", 0}
      program, args, opts -> fake_command(program, args, opts)
    end

    capture_io(fn ->
      assert_raise RuntimeError, ~r/does not match the archive ABI 2.15/, fn ->
        NativeArtifacts.build!("linux_aarch64", "0.5.1",
          root: context.root,
          output: context.output,
          run: run
        )
      end
    end)

    assert File.ls!(context.output) == []
    assert commands() == []
  end

  test "CPU and Vulkan-only dependency checks reject accidental OpenGL", context do
    for {profile, error} <- [
          {"drm,embedded-freetype", "CPU DRM"},
          {"vulkan,embedded-freetype", "Vulkan-only"}
        ] do
      run = fn
        "cargo", ["tree" | _] = args, opts ->
          if value(args, "--features") == profile,
            do: {"glutin v0.32.3", 0},
            else: fake_command("cargo", args, opts)

        program, args, opts ->
          fake_command(program, args, opts)
      end

      capture_io(fn ->
        assert_raise RuntimeError, ~r/#{error} artifact enables/, fn ->
          NativeArtifacts.build!("linux_aarch64", "0.5.1",
            root: context.root,
            output: context.output,
            run: run
          )
        end
      end)
    end
  end

  defp build!(builder, context) do
    capture_io(fn ->
      NativeArtifacts.build!(builder, "0.5.1",
        root: context.root,
        output: context.output,
        input_cache: Path.join(context.root, "input-cache"),
        run: &fake_command/3
      )
    end)
  end

  defp fake_command(program, args, opts) do
    send(self(), {:command, program, args, opts})

    case {program, args} do
      {tool, ["build" | _]} when tool in ["cargo", "cross"] ->
        target = value(args, "--target")
        directory = Path.join([Map.new(opts[:env])["CARGO_TARGET_DIR"], target, "release"])
        File.mkdir_p!(directory)
        extension = if String.ends_with?(target, "darwin"), do: "dylib", else: "so"

        File.write!(
          Path.join(directory, "libemerge_skia.#{extension}"),
          value(args, "--features")
        )

        if "macos_host" in args do
          File.write!(Path.join(directory, "macos_host"), "macos")
          File.chmod!(Path.join(directory, "macos_host"), 0o755)
        end

        {"", 0}

      {"docker", ["run" | _]} ->
        [target, profile, version, "2.15"] = Enum.take(args, -4)

        job =
          Enum.find(
            BuildProfiles.release_jobs(),
            &(&1.target == target and &1.profile == profile)
          )

        mount = Enum.find(args, &String.ends_with?(&1, ",target=/artifacts"))

        output =
          mount
          |> String.replace_prefix("type=bind,source=", "")
          |> String.replace_suffix(",target=/artifacts", "")

        File.write!(
          Path.join(output, NativeArtifacts.archive_name(job, version)),
          job.cargo_features
        )

        {"", 0}

      {"cargo", ["tree" | _]} ->
        if "--invert" in args, do: {"nif_version_2_14,nif_version_2_15", 0}, else: {"", 0}

      {"tar", _} ->
        System.cmd(program, args, opts)

      _ ->
        {"", 0}
    end
  end

  defp archive_entries(path) do
    {:ok, entries} = :erl_tar.extract(String.to_charlist(path), [:compressed, :memory])
    entries
  end

  defp value([key, value | _], key), do: value
  defp value([_ | rest], key), do: value(rest, key)

  defp commands(acc \\ []) do
    receive do
      {:command, program, args, opts} -> commands([{program, args, opts} | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end
end
