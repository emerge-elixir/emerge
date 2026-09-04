defmodule EmergeSkia.NativePrecompiledTest do
  use ExUnit.Case, async: true

  @moduletag :linux_only
  @root Path.expand("../..", __DIR__)

  test "Trellis downloads the raster archive using its existing compiler target" do
    fixture = fixture([], "")
    server = serve_archive(fixture.archive)

    assert_run(fixture, "loaded", "http://127.0.0.1:#{server.port}")

    assert Task.await(server.task, 30_000) ==
             "/releases/download/v#{fixture.version}/#{fixture.file_name}"
  end

  test "Trellis reuses the OpenGL archive without Rustler or target overrides" do
    fixture = fixture([:drm], "--opengl")
    File.write!(Path.join(fixture.cache, fixture.file_name), fixture.archive)
    assert_run(fixture, "loaded")
  end

  test "failed checksum validation restores the original Nerves target environment" do
    fixture = fixture([], "")
    File.write!(Path.join(fixture.cache, fixture.file_name), fixture.archive <> "corrupt")
    assert_run(fixture, "integrity check failed")
  end

  test "ARMv6 does not select the ARMv7 artifact" do
    fixture = fixture([], "")
    File.write!(Path.join(fixture.cache, fixture.file_name), fixture.archive)

    assert_run(fixture, "Rustler dependency is needed", nil, "armv6-nerves-linux-gnueabihf-gcc")
  end

  defp fixture(backends, suffix) do
    root =
      Path.join(System.tmp_dir!(), "emerge-native-target-#{System.unique_integer([:positive])}")

    cache = Path.join(root, "cache")
    app_dir = Path.join(root, "lib/emerge")
    version = Mix.Project.config()[:version]

    file_name =
      "libemerge_skia-v#{version}-nif-2.15-armv7-unknown-linux-gnueabihf#{suffix}.so.tar.gz"

    library_name = String.replace_suffix(file_name, ".tar.gz", "")

    File.mkdir_p!(cache)
    File.mkdir_p!(Path.join(root, "lib/emerge_skia"))
    File.mkdir_p!(Path.join(app_dir, "ebin"))
    File.mkdir_p!(Path.join(app_dir, "priv/native"))
    File.cp!(Path.join(@root, "mix.exs"), Path.join(root, "mix.exs"))

    File.cp!(
      Path.join(@root, "lib/emerge_skia/native.ex"),
      Path.join(root, "lib/emerge_skia/native.ex")
    )

    # This is a host NIF under the ARMv7 archive name: the test exercises
    # selection, checksum verification, extraction, and loading, not ARM code.
    host_nif = File.read!(Application.app_dir(:emerge, "priv/native/emerge_skia.so"))
    archive_path = Path.join(root, "fixture.tar.gz")
    :ok = :erl_tar.create(archive_path, [{to_charlist(library_name), host_nif}], [:compressed])
    archive = File.read!(archive_path)
    hash = :crypto.hash(:sha256, archive) |> Base.encode16(case: :lower)

    File.write!(
      Path.join(root, "checksum-Elixir.EmergeSkia.Native.exs"),
      inspect(%{file_name => "sha256:#{hash}"})
    )

    File.write!(Path.join(root, "probe.exs"), """
    Mix.start()
    Code.compile_file("mix.exs")
    Application.put_env(:emerge, :compiled_backends, #{inspect(backends)})
    Code.compile_file(#{inspect(Path.join(@root, "lib/emerge_skia/build_config.ex"))})
    :code.add_patha(~c"#{app_dir}/ebin")
    false = Code.ensure_loaded?(Rustler)
    "arm" = System.fetch_env!("TARGET_ARCH")
    IO.inspect(Mix.Project.config()[:rustler_opts][:target], label: "compiler_target")

    result =
      try do
        Code.compile_file("lib/emerge_skia/native.ex")
        tree = EmergeSkia.Native.tree_new()
        true = EmergeSkia.Native.tree_is_empty(tree)
        "loaded"
      rescue
        error -> Exception.message(error)
      end

    "arm" = System.fetch_env!("TARGET_ARCH")
    IO.puts("target_restored")
    IO.puts(result)
    """)

    on_exit(fn -> File.rm_rf!(root) end)
    %{root: root, cache: cache, archive: archive, file_name: file_name, version: version}
  end

  defp assert_run(fixture, expected, url \\ nil, compiler \\ "armv7-nerves-linux-gnueabihf-gcc") do
    paths =
      :code.get_path()
      |> Enum.map(&to_string/1)
      |> Enum.reject(&(Path.basename(Path.dirname(&1)) == "rustler"))
      |> Enum.flat_map(&["-pa", &1])

    {output, status} =
      System.cmd(System.find_executable("elixir"), paths ++ ["probe.exs"],
        cd: fixture.root,
        stderr_to_stdout: true,
        env: [
          {"ERL_FLAGS", "+S 2:2"},
          {"TARGET_ARCH", "arm"},
          {"TARGET_CPU", "cortex_a7"},
          {"TARGET_OS", "linux"},
          {"TARGET_ABI", "gnueabihf"},
          {"TARGET_VENDOR", nil},
          {"CC", "/opt/toolchain/bin/#{compiler}"},
          {"NERVES_SDK_SYSROOT", nil},
          {"NERVES_TOOLCHAIN", nil},
          {"EMERGE_SKIA_CHECKSUM_ONLY", nil},
          {"EMERGE_SKIA_BUILD", nil},
          {"RUSTLER_PRECOMPILED_FORCE_BUILD_ALL", nil},
          {"RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH", fixture.cache},
          {"EMERGE_SKIA_PRECOMPILED_SOURCE_URL", url || "http://127.0.0.1:1/must-not-download"},
          {"HTTP_PROXY", nil},
          {"http_proxy", nil},
          {"HTTPS_PROXY", nil},
          {"https_proxy", nil}
        ]
      )

    assert status == 0, output
    assert output =~ expected, output
    assert output =~ "target_restored", output

    target =
      if String.starts_with?(compiler, "armv7"),
        do: "armv7-unknown-linux-gnueabihf",
        else: "arm-unknown-linux-gnueabihf"

    assert output =~ ~s(compiler_target: "#{target}"), output
  end

  defp serve_archive(archive) do
    {:ok, listener} =
      :gen_tcp.listen(0, [:binary, packet: :http_bin, active: false, ip: {127, 0, 0, 1}])

    {:ok, port} = :inet.port(listener)

    task =
      Task.async(fn ->
        {:ok, socket} = :gen_tcp.accept(listener, 30_000)
        {:ok, {:http_request, :GET, {:abs_path, path}, _}} = :gen_tcp.recv(socket, 0, 5_000)
        drain_headers(socket)

        :ok =
          :gen_tcp.send(socket, [
            "HTTP/1.1 200 OK\r\nContent-Length: #{byte_size(archive)}\r\nConnection: close\r\n\r\n",
            archive
          ])

        :gen_tcp.close(socket)
        path
      end)

    on_exit(fn ->
      :gen_tcp.close(listener)
      if Process.alive?(task.pid), do: Process.exit(task.pid, :kill)
    end)

    %{task: task, port: port}
  end

  defp drain_headers(socket) do
    case :gen_tcp.recv(socket, 0, 5_000) do
      {:ok, :http_eoh} -> :ok
      {:ok, {:http_header, _, _, _, _}} -> drain_headers(socket)
    end
  end
end
