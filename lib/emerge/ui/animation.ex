defmodule Emerge.UI.Animation do
  @moduledoc "Animation helpers for runtime attribute transitions."

  @type curve :: :linear | :ease_in | :ease_out | :ease_in_out
  @type repeat :: :once | :loop | {:times, pos_integer()}
  @type keyframe :: Emerge.UI.attrs() | %{optional(atom()) => term()}
  @type keyframes :: [keyframe()]
  @type spec_map :: %{
          required(:keyframes) => keyframes(),
          required(:duration) => number(),
          required(:curve) => curve(),
          optional(:repeat) => repeat()
        }

  @type animate_attr :: {:animate, spec_map()}
  @type animate_enter_attr :: {:animate_enter, spec_map()}
  @type animate_exit_attr :: {:animate_exit, spec_map()}
  @type t :: animate_attr() | animate_enter_attr() | animate_exit_attr()

  @doc "Animate compatible attrs across keyframes"
  @spec animate(keyframes(), number(), curve()) :: animate_attr()
  @spec animate(keyframes(), number(), curve(), repeat()) :: animate_attr()
  def animate(keyframes, duration, curve, repeat \\ :once) do
    {:animate, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc """
  Animate compatible attrs once when the element is first mounted.

  Unlike `animate/4`, this does not start if it is added later to an existing retained node.
  If both `animate_enter/4` and `animate/4` are present, `animate/4` starts after the
  enter animation completes.
  """
  @spec animate_enter(keyframes(), number(), curve()) :: animate_enter_attr()
  @spec animate_enter(keyframes(), number(), curve(), repeat()) :: animate_enter_attr()
  def animate_enter(keyframes, duration, curve, repeat \\ :once) do
    {:animate_enter, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc "Animate compatible attrs once when the element is removed from the tree"
  @spec animate_exit(keyframes(), number(), curve()) :: animate_exit_attr()
  @spec animate_exit(keyframes(), number(), curve(), :once) :: animate_exit_attr()
  def animate_exit(keyframes, duration, curve, repeat \\ :once) do
    {:animate_exit, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end
end
