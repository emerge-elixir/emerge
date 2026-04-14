defmodule EmergeSkia.TreeRenderer do
  @moduledoc false

  alias EmergeSkia.Assets
  alias EmergeSkia.Options
  alias EmergeSkia.Transport

  @spec upload_tree(reference(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
  def upload_tree(renderer, tree) do
    state = Emerge.Engine.diff_state_new()
    {full_bin, state, assigned} = Emerge.Engine.encode_full(state, tree)

    case Transport.for_renderer(renderer).upload_tree(renderer, full_bin) do
      :ok -> :ok
      {:ok, :ok} -> :ok
      {:error, reason} -> raise "renderer_upload failed: #{reason}"
    end

    {state, assigned}
  end

  @spec patch_tree(reference(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
  def patch_tree(renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.Engine.diff_state_update(state, tree)

    case Transport.for_renderer(renderer).patch_tree(renderer, patch_bin) do
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
      :render_tree_to_pixels,
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
      :render_tree_to_png,
      "render_tree_to_png"
    )
  end

  defp render_offscreen(tree, opts, default_asset_timeout_ms, action, label) do
    asset_config = Assets.normalize_asset_config!(opts)
    raster_opts = Options.normalize_raster_opts!(opts, default_asset_timeout_ms)
    transport = Transport.default()

    case Assets.preload_font_assets(asset_config, transport) do
      :ok ->
        state = Emerge.Engine.diff_state_new()
        {full_bin, _state, _assigned} = Emerge.Engine.encode_full(state, tree)

        case apply(transport, action, [full_bin, raster_opts, asset_config]) do
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
