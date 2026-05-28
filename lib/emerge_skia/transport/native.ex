defmodule EmergeSkia.Transport.Native do
  @moduledoc false

  @behaviour EmergeSkia.Transport

  alias EmergeSkia.Assets
  alias EmergeSkia.Native

  @impl true
  def start_session(native_opts, asset_config) do
    case Native.start_opts(native_opts) do
      ref when is_reference(ref) ->
        case Assets.initialize_renderer_assets(ref, asset_config) do
          :ok ->
            {:ok, ref}

          {:error, reason} ->
            _ = Native.stop(ref)
            {:error, reason}
        end

      error ->
        {:error, error}
    end
  end

  @impl true
  def stop_session(renderer) do
    Native.stop(renderer)
  end

  @impl true
  def session_running?(renderer) do
    Native.is_running(renderer)
  end

  @impl true
  def set_input_target(renderer, pid) do
    Native.set_input_target(renderer, pid)
  end

  @impl true
  def set_log_target(renderer, pid) do
    Native.set_log_target(renderer, pid)
  end

  @impl true
  def stats(renderer, command) do
    Native.stats(renderer, command)
  end

  @impl true
  def renderer_info(renderer) do
    case Native.renderer_info(renderer) do
      {:ok, info} -> {:ok, normalize_renderer_info(info)}
      {:error, reason} -> {:error, reason}
    end
  end

  @impl true
  def set_input_mask(renderer, mask) do
    Native.set_input_mask(renderer, mask)
  end

  @impl true
  def upload_tree(renderer, full_bin) do
    Native.renderer_upload(renderer, full_bin)
  end

  @impl true
  def patch_tree(renderer, patch_bin) do
    Native.renderer_patch(renderer, patch_bin)
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
      renderer,
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

  defp normalize_renderer_info(info) do
    %{
      backend: info.backend |> string_to_renderer_atom(),
      backend_renderer: %{
        requested: info.backend_renderer.requested |> string_to_renderer_atom(),
        selected: info.backend_renderer.selected |> string_to_renderer_atom()
      },
      capabilities: %{
        gpu: info.capabilities.gpu,
        renderer_cache: info.capabilities.renderer_cache,
        screenshot: info.capabilities.screenshot,
        raster_present: Enum.map(info.capabilities.raster_present, &string_to_renderer_atom/1),
        prime_video: info.capabilities.prime_video
      }
    }
  end

  defp string_to_renderer_atom(value) when is_binary(value) do
    case value do
      "auto" -> :auto
      "gl" -> :gl
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
