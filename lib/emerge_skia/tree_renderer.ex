defmodule EmergeSkia.TreeRenderer do
  @moduledoc false

  alias EmergeSkia.Assets
  alias EmergeSkia.Native
  alias EmergeSkia.Options

  @spec upload_tree(reference(), Emerge.Element.t()) :: {Emerge.DiffState.t(), Emerge.Element.t()}
  def upload_tree(renderer, tree) do
    state = Emerge.diff_state_new()
    {full_bin, state, assigned} = Emerge.encode_full(state, tree)

    case Native.renderer_upload(renderer, full_bin) do
      :ok -> :ok
      {:ok, _} -> :ok
      {:error, reason} -> raise "renderer_upload failed: #{reason}"
    end

    {state, assigned}
  end

  @spec patch_tree(reference(), Emerge.DiffState.t(), Emerge.Element.t()) ::
          {Emerge.DiffState.t(), Emerge.Element.t()}
  def patch_tree(renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.diff_state_update(state, tree)

    case Native.renderer_patch(renderer, patch_bin) do
      :ok -> :ok
      {:ok, _} -> :ok
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end

    {state, assigned}
  end

  @spec render_to_pixels(Emerge.Element.t(), keyword(), pos_integer()) :: binary()
  def render_to_pixels(tree, opts, default_asset_timeout_ms) when is_list(opts) do
    opts = Options.normalize_render_to_pixels_keyword_opts!(opts)
    asset_config = Assets.normalize_asset_config!(opts)
    raster_opts = Options.normalize_raster_opts!(opts, default_asset_timeout_ms)

    with :ok <- Assets.preload_font_assets(asset_config) do
      state = Emerge.diff_state_new()
      {full_bin, _state, _assigned} = Emerge.encode_full(state, tree)

      case Native.render_tree_to_pixels_nif(
             full_bin,
             raster_opts.width,
             raster_opts.height,
             raster_opts.scale,
             [asset_config.priv_dir],
             asset_config.runtime_enabled,
             asset_config.runtime_allowlist,
             asset_config.runtime_follow_symlinks,
             asset_config.runtime_max_file_size,
             asset_config.runtime_extensions,
             raster_opts.asset_mode,
             raster_opts.asset_timeout_ms
           ) do
        pixels when is_binary(pixels) ->
          pixels

        {:ok, pixels} when is_binary(pixels) ->
          pixels

        {:error, reason} ->
          raise "render_tree_to_pixels failed: #{reason}"

        other ->
          raise "render_tree_to_pixels returned unexpected result: #{inspect(other)}"
      end
    else
      {:error, reason} ->
        raise "render_tree_to_pixels failed: #{inspect(reason)}"
    end
  end
end
