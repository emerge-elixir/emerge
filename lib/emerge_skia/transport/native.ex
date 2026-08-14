defmodule EmergeSkia.Transport.Native do
  @moduledoc false

  @behaviour EmergeSkia.Transport

  alias EmergeSkia.Assets
  alias EmergeSkia.HeadlessPrimeSession
  alias EmergeSkia.Native

  @impl true
  def start_session(native_opts, asset_config) do
    native_opts
    |> start_native_session()
    |> initialize_assets(asset_config)
  end

  @impl true
  def stop_session(%HeadlessPrimeSession{} = renderer), do: HeadlessPrimeSession.stop(renderer)

  def stop_session(renderer) do
    case Native.stop(renderer) do
      {:ok, :ok} -> :ok
      {:error, _reason} = error -> error
    end
  end

  @impl true
  def session_running?(%HeadlessPrimeSession{} = renderer),
    do: HeadlessPrimeSession.running?(renderer)

  def session_running?(renderer), do: Native.is_running(renderer)

  @impl true
  def set_input_target(renderer, pid) do
    Native.set_input_target(native_renderer(renderer), pid)
  end

  @impl true
  def set_log_target(renderer, pid) do
    Native.set_log_target(native_renderer(renderer), pid)
  end

  @impl true
  def stats(renderer, command) do
    Native.stats(native_renderer(renderer), command)
  end

  @impl true
  def renderer_info(renderer) do
    case Native.renderer_info(native_renderer(renderer)) do
      {:ok, info} -> {:ok, normalize_renderer_info(info)}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def capture_pixels(renderer, opts) do
    Native.renderer_capture_pixels(native_renderer(renderer), opts)
  end

  @impl true
  def capture_png(renderer, opts) do
    Native.renderer_capture_png(native_renderer(renderer), opts)
  end

  @impl true
  def set_input_mask(renderer, mask) do
    Native.set_input_mask(native_renderer(renderer), mask)
  end

  @impl true
  def upload_tree(renderer, full_bin) do
    Native.renderer_upload(native_renderer(renderer), full_bin)
  end

  @impl true
  def patch_tree(renderer, patch_bin) do
    Native.renderer_patch(native_renderer(renderer), patch_bin)
  end

  @impl true
  def measure_text(text, font_size) do
    Native.measure_text(text, font_size)
  end

  @impl true
  def load_font(family, weight, italic, data) do
    Native.load_font_nif(family, weight, italic, data)
  end

  @impl true
  def configure_assets(renderer, asset_config) do
    Native.configure_assets_nif(
      native_renderer(renderer),
      [asset_config.priv_dir],
      asset_config.runtime_enabled,
      asset_config.runtime_allowlist,
      asset_config.runtime_follow_symlinks,
      asset_config.runtime_max_file_size,
      asset_config.runtime_extensions
    )
  end

  @impl true
  def render_tree_to_pixels(full_bin, raster_opts, asset_config) do
    Native.render_tree_to_pixels_nif(full_bin, offscreen_opts(raster_opts, asset_config))
  end

  @impl true
  def render_tree_to_png(full_bin, raster_opts, asset_config) do
    Native.render_tree_to_png_nif(full_bin, offscreen_opts(raster_opts, asset_config))
  end

  defp start_native_session(%{backend: "headless", headless: %{mode: "prime"}} = native_opts),
    do: HeadlessPrimeSession.start(native_opts)

  defp start_native_session(native_opts) do
    case Native.start_opts(native_opts) do
      ref when is_reference(ref) -> {:ok, ref}
      error -> {:error, error}
    end
  end

  defp initialize_assets({:ok, renderer}, asset_config) do
    case Assets.initialize_renderer_assets(renderer, asset_config) do
      :ok ->
        {:ok, renderer}

      {:error, reason} ->
        _ = stop_session(renderer)
        {:error, reason}
    end
  end

  defp initialize_assets({:error, reason}, _asset_config), do: {:error, reason}

  @doc false
  def native_renderer(%HeadlessPrimeSession{renderer: renderer}), do: renderer
  def native_renderer(renderer), do: renderer

  defp normalize_renderer_info(info) do
    %{
      backend: info.backend |> string_to_renderer_atom(),
      rendering_api: %{
        requested: info.rendering_api.requested |> string_to_renderer_atom(),
        selected: info.rendering_api.selected |> string_to_renderer_atom()
      },
      capabilities: %{
        gpu: info.capabilities.gpu,
        renderer_cache: info.capabilities.renderer_cache,
        screenshot: info.capabilities.screenshot,
        raster_present: Enum.map(info.capabilities.raster_present, &string_to_renderer_atom/1),
        prime_video: info.capabilities.prime_video,
        prime_video_formats: info.capabilities.prime_video_formats
      },
      vulkan_device: normalize_vulkan_device(info.vulkan_device)
    }
  end

  defp normalize_vulkan_device(nil), do: nil

  defp normalize_vulkan_device(device) do
    %{
      physical_device_name: device.physical_device_name,
      driver_name: device.driver_name,
      driver_id: device.driver_id,
      software: device.software,
      drm_node: normalize_vulkan_drm_node(device.drm_node)
    }
  end

  defp normalize_vulkan_drm_node(nil), do: nil

  defp normalize_vulkan_drm_node(node) do
    %{
      path: node.path,
      match_field: string_to_drm_match_field(node.match_field),
      major: node.major,
      minor: node.minor
    }
  end

  defp string_to_drm_match_field("primary"), do: :primary
  defp string_to_drm_match_field("render"), do: :render
  defp string_to_drm_match_field(value), do: value

  defp string_to_renderer_atom(value) when is_binary(value) do
    case value do
      "auto" -> :auto
      "opengl" -> :opengl
      "raster" -> :raster
      "metal" -> :metal
      "vulkan" -> :vulkan
      "wayland" -> :wayland
      "drm" -> :drm
      "macos" -> :macos
      "headless" -> :headless
      "gpu_upload" -> :gpu_upload
      "cpu" -> :cpu
      other -> String.to_atom(other)
    end
  end

  defp offscreen_opts(raster_opts, asset_config) do
    %{
      width: raster_opts.width,
      height: raster_opts.height,
      scale: raster_opts.scale,
      sources: [asset_config.priv_dir],
      runtime_enabled: asset_config.runtime_enabled,
      allowlist: asset_config.runtime_allowlist,
      follow_symlinks: asset_config.runtime_follow_symlinks,
      max_file_size: asset_config.runtime_max_file_size,
      extensions: asset_config.runtime_extensions,
      asset_mode: raster_opts.asset_mode,
      asset_timeout_ms: raster_opts.asset_timeout_ms
    }
  end
end
