defmodule Emerge.UI.Animation do
  @moduledoc "Animation helpers for runtime attribute transitions."

  @doc "Animate compatible attrs across keyframes"
  def animate(keyframes, duration, curve, repeat \\ :once) do
    {:animate, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc """
  Animate compatible attrs once when the element is first mounted.

  Unlike `animate/4`, this does not start if it is added later to an existing retained node.
  If both `animate_enter/4` and `animate/4` are present, `animate/4` starts after the
  enter animation completes.
  """
  def animate_enter(keyframes, duration, curve, repeat \\ :once) do
    {:animate_enter, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc "Animate compatible attrs once when the element is removed from the tree"
  def animate_exit(keyframes, duration, curve, repeat \\ :once) do
    {:animate_exit, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end
end
