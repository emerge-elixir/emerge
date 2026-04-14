defmodule EmergeSkia.Macos.Launcher do
  @moduledoc false

  alias EmergeSkia.BuildConfig

  @host_socket_env "EMERGE_SKIA_MACOS_HOST_SOCKET"
  @host_binary_env "EMERGE_SKIA_MACOS_HOST_BINARY"
  @download_timeout_ms 60_000
  @lock_retry_ms 100
  @lock_timeout_ms 30_000

  def prepare do
    case System.get_env(@host_socket_env) do
      socket_path when is_binary(socket_path) and socket_path != "" ->
        {:ok, %{socket_path: socket_path, port: nil, launched?: false}}

      _ ->
        socket_path = default_socket_path()
        _ = File.rm(socket_path)

        with {:ok, host_binary} <- host_binary_path(),
             {:ok, port} <- launch_host(socket_path, host_binary) do
          {:ok, %{socket_path: socket_path, port: port, launched?: true}}
        end
    end
  end

  def host_binary_path do
    case System.get_env(@host_binary_env) do
      path when is_binary(path) and path != "" ->
        if File.regular?(path) do
          {:ok, path}
        else
          {:error, "configured macOS host binary does not exist at #{path}"}
        end

      _ ->
        resolve_host_binary()
    end
  end

  def project_root do
    Path.expand("../../..", __DIR__)
  end

  defp resolve_host_binary do
    target = BuildConfig.macos_host_target()
    cached_binary = Path.join(BuildConfig.macos_host_cache_dir(target), "macos_host")
    bundled_binary = Path.join(project_root(), "priv/native/macos_host")

    cond do
      File.regular?(cached_binary) ->
        {:ok, cached_binary}

      File.regular?(bundled_binary) ->
        {:ok, bundled_binary}

      true ->
        ensure_downloaded_host_binary(target, cached_binary)
    end
  end

  defp ensure_downloaded_host_binary(target, cached_binary) do
    cache_dir = Path.dirname(cached_binary)

    with :ok <- File.mkdir_p(cache_dir),
         :ok <-
           with_download_lock(cache_dir, fn -> download_host_binary(target, cached_binary) end) do
      if File.regular?(cached_binary) do
        {:ok, cached_binary}
      else
        maybe_build_local_host(target)
      end
    else
      {:error, reason} ->
        maybe_build_local_host(target, reason)
    end
  end

  defp maybe_build_local_host(_target, prior_reason \\ nil) do
    if System.get_env(BuildConfig.build_local_macos_host_env_key()) in ["1", "true"] do
      host_binary = local_dev_binary_path()

      case build_host_binary(host_binary) do
        :ok -> {:ok, host_binary}
        {:error, reason} -> {:error, reason}
      end
    else
      message =
        [
          "macOS host binary is unavailable and local build fallback is disabled",
          prior_reason && "reason: #{prior_reason}",
          "set #{BuildConfig.build_local_macos_host_env_key()}=true to allow local cargo builds during development"
        ]
        |> Enum.reject(&is_nil/1)
        |> Enum.join("\n")

      {:error, message}
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

  defp download_host_binary(target, cached_binary) do
    if File.regular?(cached_binary) do
      :ok
    else
      archive_name = BuildConfig.macos_host_archive_name(target)

      tmp_root =
        Path.join(Path.dirname(cached_binary), ".download-#{System.unique_integer([:positive])}")

      archive_path = Path.join(tmp_root, archive_name)
      checksum_path = Path.join(tmp_root, "#{archive_name}.sha256")
      extract_dir = Path.join(tmp_root, "extract")

      with :ok <- File.mkdir_p(extract_dir),
           :ok <- download_to_path(BuildConfig.macos_host_download_url(target), archive_path),
           :ok <- download_to_path(BuildConfig.macos_host_checksum_url(target), checksum_path),
           :ok <- verify_checksum(archive_path, checksum_path),
           :ok <- extract_archive(archive_path, extract_dir),
           :ok <- install_extracted_binary(extract_dir, cached_binary) do
        File.rm_rf(tmp_root)
        :ok
      else
        {:error, reason} ->
          _ = File.rm_rf(tmp_root)
          {:error, reason}
      end
    end
  end

  defp download_to_path(request, destination) do
    :inets.start()
    :ssl.start()

    case :httpc.request(:get, http_request(request), [timeout: @download_timeout_ms],
           body_format: :binary
         ) do
      {:ok, {{_, 200, _}, _headers, body}} ->
        File.write(destination, body)

      {:ok, {{_, status, _}, _headers, body}} ->
        {:error,
         "failed to download macOS host artifact (HTTP #{status}): #{String.trim(to_string(body))}"}

      {:error, reason} ->
        {:error, "failed to download macOS host artifact: #{inspect(reason)}"}
    end
  end

  defp verify_checksum(archive_path, checksum_path) do
    with {:ok, checksum_contents} <- File.read(checksum_path),
         [expected | _] <- String.split(String.trim(checksum_contents)),
         {:ok, archive_contents} <- File.read(archive_path) do
      actual = :crypto.hash(:sha256, archive_contents) |> Base.encode16(case: :lower)

      if String.downcase(expected) == actual do
        :ok
      else
        {:error, "macOS host archive checksum mismatch"}
      end
    else
      [] -> {:error, "macOS host checksum file is empty"}
      {:error, reason} -> {:error, "failed to read macOS host artifact: #{inspect(reason)}"}
    end
  end

  defp extract_archive(archive_path, extract_dir) do
    case :erl_tar.extract(String.to_charlist(archive_path), [
           :compressed,
           {:cwd, String.to_charlist(extract_dir)}
         ]) do
      :ok -> :ok
      {:error, reason} -> {:error, "failed to extract macOS host archive: #{inspect(reason)}"}
    end
  end

  defp install_extracted_binary(extract_dir, cached_binary) do
    extracted_binary = Path.join(extract_dir, "macos_host")

    with true <- File.regular?(extracted_binary) || {:error, :missing_binary},
         :ok <- File.chmod(extracted_binary, 0o755) do
      _ = File.rm(cached_binary)
      File.rename(extracted_binary, cached_binary)
    else
      {:error, :missing_binary} ->
        {:error, "macOS host archive did not contain macos_host"}

      {:error, reason} ->
        {:error, "failed to install macOS host binary: #{inspect(reason)}"}
    end
  end

  defp http_request({url, headers}) do
    {String.to_charlist(url),
     Enum.map(headers, fn {k, v} -> {String.to_charlist(k), String.to_charlist(v)} end)}
  end

  defp http_request(url) when is_binary(url), do: {String.to_charlist(url), []}

  defp with_download_lock(cache_dir, fun) do
    lock_dir = Path.join(cache_dir, ".download.lock")
    deadline = System.monotonic_time(:millisecond) + @lock_timeout_ms
    acquire_download_lock(lock_dir, deadline, fun)
  end

  defp acquire_download_lock(lock_dir, deadline, fun) do
    case File.mkdir(lock_dir) do
      :ok ->
        try do
          fun.()
        after
          _ = File.rmdir(lock_dir)
        end

      {:error, :eexist} ->
        if System.monotonic_time(:millisecond) < deadline do
          Process.sleep(@lock_retry_ms)
          acquire_download_lock(lock_dir, deadline, fun)
        else
          {:error, "timed out waiting for macOS host download lock"}
        end

      {:error, reason} ->
        {:error, "failed to acquire macOS host download lock: #{inspect(reason)}"}
    end
  end

  defp local_dev_binary_path do
    Path.join(project_root(), "native/emerge_skia/target/debug/macos_host")
  end

  defp launch_host(socket_path, host_binary) do
    port =
      Port.open(
        {:spawn_executable, host_binary},
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
