defmodule Emerge.Viewport.Renderer.Skia do
  @moduledoc false

  @behaviour Emerge.Viewport.Renderer

  @impl true
  def start(skia_opts, _renderer_opts) when is_list(skia_opts), do: EmergeSkia.start(skia_opts)

  @impl true
  def stop(renderer), do: EmergeSkia.stop(renderer)

  @impl true
  def running?(renderer), do: EmergeSkia.running?(renderer)

  @impl true
  def set_input_target(renderer, pid), do: EmergeSkia.set_input_target(renderer, pid)

  @impl true
  def set_input_mask(renderer, mask), do: EmergeSkia.set_input_mask(renderer, mask)

  @impl true
  def upload_tree(renderer, tree), do: EmergeSkia.upload_tree(renderer, tree)

  @impl true
  def patch_tree(renderer, diff_state, tree),
    do: EmergeSkia.patch_tree(renderer, diff_state, tree)
end
