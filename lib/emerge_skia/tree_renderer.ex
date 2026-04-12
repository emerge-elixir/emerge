defmodule EmergeSkia.TreeRenderer do
  @moduledoc false

  alias EmergeSkia.Assets
  alias EmergeSkia.Macos.Host
  alias EmergeSkia.Macos.Renderer
  alias EmergeSkia.Native
  alias EmergeSkia.Options

  @spec upload_tree(reference(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
  def upload_tree(%Renderer{} = renderer, tree) do
    state = Emerge.Engine.diff_state_new()
    {full_bin, state, assigned} = Emerge.Engine.encode_full(state, tree)

    case Host.upload_tree(renderer, full_bin) do
      :ok -> {state, assigned}
      {:error, reason} -> raise "renderer_upload failed: #{reason}"
    end
  end

  def upload_tree(renderer, tree) do
    state = Emerge.Engine.diff_state_new()
    {full_bin, state, assigned} = Emerge.Engine.encode_full(state, tree)

    case Native.renderer_upload(renderer, full_bin) do
      :ok -> :ok
      {:ok, :ok} -> :ok
      {:error, reason} -> raise "renderer_upload failed: #{reason}"
    end

    {state, assigned}
  end

  @spec patch_tree(reference(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
  def patch_tree(%Renderer{} = renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.Engine.diff_state_update(state, tree)

    case Host.patch_tree(renderer, patch_bin) do
      :ok -> {state, assigned}
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end
  end

  def patch_tree(renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.Engine.diff_state_update(state, tree)

    case Native.renderer_patch(renderer, patch_bin) do
      :ok -> :ok
      {:ok, :ok} -> :ok
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end

    {state, assigned}
  end

  @spec render_to_pixels(Emerge.Engine.Element.t(), keyword(), pos_integer()) :: binary()
  def render_to_pixels(tree, opts, default_asset_timeout_ms) when is_list(opts) do
    opts = Options.normalize_render_to_pixels_keyword_opts!(opts)

    render_offscreen(
      tree,
      opts,
      default_asset_timeout_ms,
      &Native.render_tree_to_pixels_nif/2,
      "render_tree_to_pixels"
    )
  end

  @spec render_to_png(Emerge.Engine.Element.t(), keyword(), pos_integer()) :: binary()
  def render_to_png(tree, opts, default_asset_timeout_ms) when is_list(opts) do
    opts = Options.normalize_render_to_png_keyword_opts!(opts)

    render_offscreen(
      tree,
      opts,
      default_asset_timeout_ms,
      &Native.render_tree_to_png_nif/2,
      "render_tree_to_png"
    )
  end

  defp render_offscreen(tree, opts, default_asset_timeout_ms, native_fun, label) do
    asset_config = Assets.normalize_asset_config!(opts)
    raster_opts = Options.normalize_raster_opts!(opts, default_asset_timeout_ms)

    case Assets.preload_font_assets(asset_config) do
      :ok ->
        state = Emerge.Engine.diff_state_new()
        {full_bin, _state, _assigned} = Emerge.Engine.encode_full(state, tree)

        case native_fun.(full_bin, %{
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
             }) do
          binary when is_binary(binary) ->
            binary

          {:ok, binary} when is_binary(binary) ->
            binary

          {:error, reason} ->
            raise "#{label} failed: #{reason}"
        end

      {:error, reason} ->
        raise "#{label} failed: #{inspect(reason)}"
    end
  end
end
