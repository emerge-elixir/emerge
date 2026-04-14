defmodule EmergeSkia.Transport.MacosHost do
  @moduledoc false

  @behaviour EmergeSkia.Transport

  alias EmergeSkia.Macos.Host

  @impl true
  def start_session(native_opts, asset_config) do
    Host.start_session(native_opts, asset_config)
  end

  @impl true
  def stop_session(renderer) do
    Host.stop_session(renderer)
  end

  @impl true
  def session_running?(renderer) do
    Host.running?(renderer)
  end

  @impl true
  def set_input_target(renderer, pid) do
    Host.set_input_target(renderer, pid)
  end

  @impl true
  def set_log_target(renderer, pid) do
    Host.set_log_target(renderer, pid)
  end

  @impl true
  def set_input_mask(renderer, mask) do
    Host.set_input_mask(renderer, mask)
  end

  @impl true
  def upload_tree(renderer, full_bin) do
    Host.upload_tree(renderer, full_bin)
  end

  @impl true
  def patch_tree(renderer, patch_bin) do
    Host.patch_tree(renderer, patch_bin)
  end

  @impl true
  def measure_text(text, font_size) do
    Host.measure_text(text, font_size)
  end

  @impl true
  def load_font(family, weight, italic, data) do
    Host.load_font(family, weight, italic, data)
  end

  @impl true
  def configure_assets(renderer, asset_config) do
    Host.configure_assets(renderer, asset_config)
  end

  @impl true
  def render_tree_to_pixels(full_bin, raster_opts, asset_config) do
    Host.render_tree_to_pixels(full_bin, raster_opts, asset_config)
  end

  @impl true
  def render_tree_to_png(full_bin, raster_opts, asset_config) do
    Host.render_tree_to_png(full_bin, raster_opts, asset_config)
  end
end
