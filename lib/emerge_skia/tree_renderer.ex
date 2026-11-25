defmodule EmergeSkia.TreeRenderer do
  @moduledoc false

  alias EmergeSkia.Assets
  alias EmergeSkia.Options
  alias EmergeSkia.Transport

  @spec upload_tree(reference(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
  def upload_tree(renderer, tree), do: upload(renderer, tree, nil)

  @doc false
  def upload(renderer, tree, relay) do
    state = Emerge.Engine.diff_state_new()
    {full_bin, state, assigned} = Emerge.Engine.encode_full(state, tree)

    if is_reference(renderer) do
      accept(renderer, nil, state, full_bin, not is_nil(relay), relay, assigned, :renderer_upload)
    else
      case Transport.for_renderer(renderer).upload_tree(renderer, full_bin) do
        :ok -> {state, assigned}
        {:error, reason} -> raise "renderer_upload failed: #{reason}"
      end
    end
  end

  @spec patch_tree(reference(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t() | nil) ::
          {Emerge.Engine.diff_state(), Emerge.Engine.Element.t() | nil}
  def patch_tree(renderer, state, tree) do
    {patch_bin, state, assigned} = Emerge.Engine.diff_state_update(state, tree)

    if is_reference(renderer) do
      accept(renderer, state.accepted, state, patch_bin, false, nil, assigned, :renderer_patch)
    else
      case Transport.for_renderer(renderer).patch_tree(renderer, patch_bin) do
        :ok -> {state, assigned}
        {:error, reason} -> raise "renderer_patch failed: #{reason}"
      end
    end
  end

  @spec patch_tree_runtime(reference(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t()) ::
          {Emerge.Engine.diff_state(), nil}
  def patch_tree_runtime(renderer, state, tree, relay \\ nil)

  def patch_tree_runtime(renderer, state, tree, relay) when is_reference(renderer) do
    {bytes, next, edits} = Emerge.Engine.DiffState.diff_and_encode_routed(state, tree)

    accept(
      renderer,
      state.accepted,
      next,
      bytes,
      map_size(edits) != 0,
      relay,
      nil,
      :renderer_patch
    )
  end

  def patch_tree_runtime(renderer, state, tree, _relay) do
    {bytes, state} = Emerge.Engine.diff_state_update_binary(state, tree)

    case Transport.for_renderer(renderer).patch_tree(renderer, bytes) do
      :ok -> {state, nil}
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end
  end

  defp accept(renderer, base, candidate, bytes, routes?, relay, assigned, action) do
    case EmergeSkia.Native.submit_revision(renderer, base, bytes, routes?, relay) do
      {:ok, {_, revision} = token} ->
        epoch = if routes?, do: revision, else: candidate.route_epoch
        {%{candidate | accepted: token, route_epoch: epoch}, assigned}

      {:error, :busy} when is_pid(relay) ->
        {:error, :busy}

      {:error, reason} ->
        raise "#{action} failed: #{reason}"
    end
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
    asset_config = Assets.normalize_asset_config!(opts, decode_at_size: true)
    raster_opts = Options.normalize_raster_opts!(opts, default_asset_timeout_ms)
    transport = Transport.default()

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
  end
end
