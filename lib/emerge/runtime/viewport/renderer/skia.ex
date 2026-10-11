defmodule Emerge.Runtime.Viewport.Renderer.Skia do
  @moduledoc false

  @behaviour Emerge.Runtime.Viewport.Renderer

  @impl true
  def start(%Emerge.Config{} = config, renderer_opts),
    do:
      EmergeSkia.start_owned(
        config,
        Keyword.get(renderer_opts, :owner),
        Keyword.get(renderer_opts, :backlight_state)
      )

  @impl true
  def configure(renderer, previous, config) do
    case EmergeSkia.configure(renderer, previous, config) do
      {:ok, token} -> {:pending, token}
      result -> result
    end
  end

  @impl true
  def stop(renderer), do: EmergeSkia.stop(renderer)

  @impl true
  def running?(renderer), do: EmergeSkia.running?(renderer)

  @impl true
  def set_input_target(renderer, pid), do: EmergeSkia.set_input_target(renderer, pid)

  @impl true
  def set_log_target(renderer, pid), do: EmergeSkia.set_log_target(renderer, pid)

  @impl true
  def set_input_mask(renderer, mask) do
    renderer |> EmergeSkia.Transport.for_renderer() |> apply(:set_input_mask, [renderer, mask])
  end

  @impl true
  def upload_tree(renderer, tree), do: EmergeSkia.upload_tree(renderer, tree)

  @impl true
  def patch_tree(renderer, diff_state, tree),
    do: EmergeSkia.patch_tree(renderer, diff_state, tree)

  @impl true
  def patch_tree_runtime(renderer, diff_state, tree),
    do: EmergeSkia.TreeRenderer.patch_tree_runtime(renderer, diff_state, tree)
end
