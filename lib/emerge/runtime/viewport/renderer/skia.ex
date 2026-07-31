defmodule Emerge.Runtime.Viewport.Renderer.Skia do
  @moduledoc false

  @behaviour Emerge.Runtime.Viewport.Renderer

  @impl true
  def start(skia_opts, _renderer_opts) when is_list(skia_opts), do: EmergeSkia.start(skia_opts)

  @impl true
  def stop(renderer), do: EmergeSkia.stop(renderer)

  @impl true
  def running?(renderer), do: EmergeSkia.running?(renderer)

  @impl true
  def set_input_target(renderer, pid), do: EmergeSkia.set_input_target(renderer, pid)

  @impl true
  def set_log_target(renderer, pid), do: EmergeSkia.set_log_target(renderer, pid)

  @impl true
  def set_input_mask(renderer, mask), do: EmergeSkia.set_input_mask(renderer, mask)

  @impl true
  def connect_video_output(%EmergeSkia.HeadlessPrimeSession{} = renderer, target, opts),
    do: EmergeSkia.HeadlessPrimeSession.connect(renderer, target, opts)

  def connect_video_output(_renderer, _target, _opts), do: {:error, :wrong_mode}

  @impl true
  def disconnect_video_output(%EmergeSkia.HeadlessPrimeSession{} = renderer),
    do: EmergeSkia.HeadlessPrimeSession.disconnect(renderer)

  def disconnect_video_output(_renderer), do: {:error, :wrong_mode}

  @impl true
  def upload_tree(renderer, tree), do: EmergeSkia.upload_tree(renderer, tree)

  @impl true
  def patch_tree(renderer, diff_state, tree),
    do: EmergeSkia.patch_tree(renderer, diff_state, tree)

  @impl true
  def patch_tree_runtime(renderer, diff_state, tree),
    do: EmergeSkia.TreeRenderer.patch_tree_runtime(renderer, diff_state, tree)
end
