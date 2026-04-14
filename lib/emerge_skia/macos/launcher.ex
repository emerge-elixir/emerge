defmodule EmergeSkia.Macos.Launcher do
  @moduledoc false

  @host_socket_env "EMERGE_SKIA_MACOS_HOST_SOCKET"
  @host_binary_env "EMERGE_SKIA_MACOS_HOST_BINARY"

  def prepare do
    case System.get_env(@host_socket_env) do
      socket_path when is_binary(socket_path) and socket_path != "" ->
        {:ok, %{socket_path: socket_path, port: nil, launched?: false}}

      _ ->
        socket_path = default_socket_path()
        _ = File.rm(socket_path)

        with :ok <- ensure_host_binary_built(),
             {:ok, port} <- launch_host(socket_path) do
          {:ok, %{socket_path: socket_path, port: port, launched?: true}}
        end
    end
  end

  def host_binary_path do
    case System.get_env(@host_binary_env) do
      path when is_binary(path) and path != "" ->
        path

      _ ->
        priv_binary = Path.join(project_root(), "priv/native/macos_host")

        if File.regular?(priv_binary) do
          priv_binary
        else
          Path.join(project_root(), "native/emerge_skia/target/debug/macos_host")
        end
    end
  end

  def project_root do
    Path.expand("../../..", __DIR__)
  end

  defp ensure_host_binary_built do
    host_binary = host_binary_path()

    if File.regular?(host_binary) do
      :ok
    else
      build_host_binary(host_binary)
    end
  end

  defp build_host_binary(host_binary) do
    cargo = System.find_executable("cargo")
    mise = System.find_executable("mise") || "/usr/local/bin/mise"
    project_root = project_root()

    {command, args} =
      if cargo do
        {cargo,
         [
           "build",
           "--manifest-path",
           Path.join(project_root, "native/emerge_skia/Cargo.toml"),
           "--bin",
           "macos_host",
           "--no-default-features",
           "--features",
           "macos"
         ]}
      else
        {mise,
         [
           "x",
           "--",
           "cargo",
           "build",
           "--manifest-path",
           Path.join(project_root, "native/emerge_skia/Cargo.toml"),
           "--bin",
           "macos_host",
           "--no-default-features",
           "--features",
           "macos"
         ]}
      end

    case System.cmd(command, args, stderr_to_stdout: true) do
      {_output, 0} ->
        if File.regular?(host_binary) do
          :ok
        else
          {:error, "macOS host build succeeded but binary was not found at #{host_binary}"}
        end

      {output, status} ->
        {:error, "failed to build macOS host (status #{status}):\n#{output}"}
    end
  end

  defp launch_host(socket_path) do
    port =
      Port.open(
        {:spawn_executable, host_binary_path()},
        [
          :binary,
          :exit_status,
          :use_stdio,
          :hide,
          args: ["--socket", socket_path, "--monitor-stdin"]
        ]
      )

    {:ok, port}
  rescue
    error -> {:error, "failed to launch macOS host: #{Exception.message(error)}"}
  end

  defp default_socket_path do
    Path.join(System.tmp_dir!(), "emerge_skia_macos_#{System.unique_integer([:positive])}.sock")
  end
end
