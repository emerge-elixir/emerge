defmodule EmergeSkia.FrameCapConfigTest do
  use ExUnit.Case, async: false

  alias Emerge.Config
  alias Emerge.Config.Normalize

  @renderer_path Path.expand("../../lib/emerge_skia.ex", __DIR__)

  test "frame caps compile without warnings and respect each default backend" do
    priv_dir = :code.priv_dir(:emerge)

    for {backend, supported?} <- [wayland: true, macos: false, drm: false] do
      renderer = compile_renderer(backend)
      assert :code.priv_dir(:emerge) == priv_dir

      for {output, supported?} <- [
            {%Config.Window{}, supported?},
            {%Config.Window{backend: :wayland}, true},
            {%Config.Window{backend: :macos}, false},
            {%Config.DRM{}, false},
            {%Config.Headless{target: self()}, true}
          ] do
        previous = Normalize.config!(%Config{output: output})
        capped = %Config{previous | renderer: %Config.Renderer{target_fps: 30}}

        assert :ok = renderer.validate_config!(previous)

        if supported? do
          assert :ok = renderer.validate_config!(capped)
          assert :ok = renderer.validate_config_change(previous, capped)
          assert :ok = renderer.validate_config_change(capped, previous)
        else
          assert_raise ArgumentError,
                       ~r/target_fps is currently supported only by headless and Wayland/,
                       fn -> renderer.validate_config!(capped) end

          assert {:error, {:unsupported_live_config, :target_fps}} =
                   renderer.validate_config_change(previous, capped)

          assert {:error, {:unsupported_live_config, :target_fps}} =
                   renderer.validate_config_change(capped, previous)
        end
      end
    end
  end

  defp compile_renderer(backend) do
    platform = backend |> Atom.to_string() |> Macro.camelize()
    build_config = Module.concat(__MODULE__, "#{platform}BuildConfig")
    renderer = Module.concat(__MODULE__, "#{platform}Renderer")

    directory =
      Path.join(System.tmp_dir!(), "frame-cap-config-#{System.unique_integer([:positive])}")

    File.mkdir_p!(directory)

    on_exit(fn ->
      for module <- [renderer, build_config] do
        :code.purge(module)
        :code.delete(module)
      end

      File.rm_rf!(directory)
    end)

    source =
      @renderer_path
      |> File.read!()
      |> String.replace("defmodule EmergeSkia do", "defmodule #{inspect(renderer)} do")
      |> String.replace("EmergeSkia.BuildConfig", inspect(build_config))

    build_config_path = Path.join(directory, "build_config.ex")
    renderer_path = Path.join(directory, "renderer.ex")

    File.write!(build_config_path, """
    defmodule #{inspect(build_config)} do
      def default_runtime_backend, do: #{inspect(backend)}
    end
    """)

    File.write!(renderer_path, source)

    # Mix disables signature inference for tests; enable it to catch platform-only warnings.
    previous = Code.get_compiler_option(:infer_signatures)
    Code.put_compiler_option(:infer_signatures, true)

    try do
      assert {:ok, _modules, %{compile_warnings: [], runtime_warnings: []}} =
               Kernel.ParallelCompiler.compile(
                 [build_config_path, renderer_path],
                 return_diagnostics: true
               )
    after
      Code.put_compiler_option(:infer_signatures, previous)
    end

    renderer
  end
end
