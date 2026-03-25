defmodule EmergeSkia.OptionsTest do
  use ExUnit.Case, async: true

  alias Emerge.Assets.Ref
  alias EmergeSkia.Assets
  alias EmergeSkia.BuildConfig
  alias EmergeSkia.Options

  test "build_start_native_opts! defaults backend from build config" do
    expected_backend = Atom.to_string(BuildConfig.default_runtime_backend())

    assert %{
             backend: ^expected_backend,
             drm_startup_retries: 40,
             drm_retry_interval_ms: 250
           } = Options.build_start_native_opts!([])
  end

  test "build_start_native_opts! keeps explicit backend selection" do
    assert %{backend: "drm"} = Options.build_start_native_opts!(backend: :drm)
    assert %{backend: "wayland"} = Options.build_start_native_opts!(backend: "wayland")
  end

  test "build_start_native_opts! validates drm retry options" do
    assert %{drm_startup_retries: 5, drm_retry_interval_ms: 100} =
             Options.build_start_native_opts!(drm_startup_retries: 5, drm_retry_interval_ms: 100)

    assert_raise ArgumentError, ~r/:drm_startup_retries must be a non-negative integer/, fn ->
      Options.build_start_native_opts!(drm_startup_retries: -1)
    end

    assert_raise ArgumentError, ~r/:drm_retry_interval_ms must be a non-negative integer/, fn ->
      Options.build_start_native_opts!(drm_retry_interval_ms: -1)
    end
  end

  test "normalize_drm_cursor_overrides! normalizes logical and runtime sources" do
    runtime_path =
      Path.join(System.tmp_dir!(), "emerge_cursor_#{System.unique_integer([:positive])}.svg")

    on_exit(fn ->
      File.rm(runtime_path)
    end)

    File.write!(runtime_path, ~S(<svg width="1" height="1" xmlns="http://www.w3.org/2000/svg"/>))

    assert [
             %{
               icon: "default",
               source: "demo_images/tile_quad.svg",
               hotspot_x: 1.0,
               hotspot_y: 1.0
             },
             %{
               icon: "text",
               source: ^runtime_path,
               hotspot_x: 11.5,
               hotspot_y: 11.5
             }
           ] =
             Assets.normalize_drm_cursor_overrides!(
               drm_cursor: [
                 default: [source: %Ref{path: "demo_images/tile_quad.svg"}, hotspot: {1, 1}],
                 text: [source: runtime_path, hotspot: {11.5, 11.5}]
               ]
             )
  end

  test "normalize_drm_cursor_overrides! accepts string keyed maps" do
    assert [
             %{
               icon: "pointer",
               source: "demo_images/tile_quad.svg",
               hotspot_x: 7.0,
               hotspot_y: 2.0
             }
           ] =
             Assets.normalize_drm_cursor_overrides!(
               drm_cursor: %{
                 "pointer" => %{"source" => "demo_images/tile_quad.svg", "hotspot" => {7, 2}}
               }
             )
  end

  test "normalize_drm_cursor_overrides! rejects unsupported extensions" do
    assert_raise ArgumentError, ~r/drm_cursor\.default\.source extension must be one of/, fn ->
      Assets.normalize_drm_cursor_overrides!(
        drm_cursor: [default: [source: "demo_images/static.jpg", hotspot: {1, 1}]]
      )
    end
  end
end
