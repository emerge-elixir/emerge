if :os.type() != {:unix, :darwin} do
  raise "metal_probe.exs must be run on macOS"
end

defmodule MetalProbeScript do
  def run do
    IO.puts("== Metal device probe ==")

    run_cmd!("build", [
      "x",
      "--",
      "cargo",
      "build",
      "--manifest-path",
      "native/emerge_skia/Cargo.toml",
      "--bin",
      "metal_probe",
      "--no-default-features",
      "--features",
      "macos"
    ])

    probe = Path.join(File.cwd!(), "native/emerge_skia/target/debug/metal_probe")

    IO.puts("\n-- BEAM System.cmd launch --")
    run_binary!(probe)

    IO.puts("\n-- BEAM Port launch --")
    run_port!(probe)
  end

  defp run_cmd!(label, args) do
    mise = System.find_executable("mise") || "/usr/local/bin/mise"
    IO.puts("$ #{mise} #{Enum.join(args, " ")}")

    case System.cmd(mise, args, stderr_to_stdout: true, into: IO.stream(:stdio, :line)) do
      {_, 0} -> :ok
      {_, status} -> raise "#{label} failed with status #{status}"
    end
  end

  defp run_binary!(path) do
    case System.cmd(path, [], stderr_to_stdout: true, into: IO.stream(:stdio, :line)) do
      {_, 0} -> :ok
      {_, status} -> raise "probe failed with status #{status}"
    end
  end

  defp run_port!(path) do
    port =
      Port.open({:spawn_executable, path}, [
        :binary,
        :exit_status,
        :use_stdio,
        :stderr_to_stdout,
        :hide
      ])

    collect_port(port)
  end

  defp collect_port(port) do
    receive do
      {^port, {:data, data}} ->
        IO.write(data)
        collect_port(port)

      {^port, {:exit_status, 0}} ->
        :ok

      {^port, {:exit_status, status}} ->
        raise "port probe failed with status #{status}"
    after
      10_000 ->
        Port.close(port)
        raise "timed out waiting for port probe output"
    end
  end
end

MetalProbeScript.run()
