if :os.type() != {:unix, :darwin} do
  raise "macos_wrapper.exs must be run on macOS"
end

defmodule MacosWrapperSmokeScript do
  def run do
    mise = System.find_executable("mise") || "/usr/local/bin/mise"
    project_root = File.cwd!()
    cargo_manifest = Path.join(project_root, "native/emerge_skia/Cargo.toml")
    wrapper_binary = Path.join(project_root, "native/emerge_skia/target/release/macos_wrapper_smoke")

    run_cmd!(mise, ["install"])

    run_cmd!(mise, [
      "x",
      "--",
      "cargo",
      "build",
      "--manifest-path",
      cargo_manifest,
      "--bin",
      "macos_host",
      "--bin",
      "macos_wrapper_smoke",
      "--no-default-features",
      "--features",
      "macos",
      "--release"
    ])

    run_cmd!(wrapper_binary, [])
    IO.puts("macOS wrapper smoke script passed")
  end

  defp run_cmd!(command, args) do
    IO.puts("$ #{command} #{Enum.join(args, " ")}")

    case System.cmd(command, args,
           stderr_to_stdout: true,
           into: IO.stream(:stdio, :line)
         ) do
      {_, 0} -> :ok
      {_, status} -> raise "command failed with status #{status}: #{command} #{Enum.join(args, " ")}"
    end
  end
end

MacosWrapperSmokeScript.run()
