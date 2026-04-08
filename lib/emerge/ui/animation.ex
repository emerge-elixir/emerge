defmodule Emerge.UI.Animation do
  @moduledoc """
  Animation helpers for declarative runtime transitions.

  ## Basics

  Animation keyframes are written with ordinary Emerge attrs. When written
  inline, keyframes are attr lists such as
  `[Transform.move_x(-40), Transform.alpha(0.0)]`. Maps are also accepted when
  building specs programmatically.

  Each animation spec needs:

  - at least 2 non-empty keyframes
  - a positive duration in milliseconds
  - a curve: `:linear`, `:ease_in`, `:ease_out`, or `:ease_in_out`
  - an optional repeat: `:once`, `:loop`, or a positive integer

  A positive integer runs that many times and then clamps to the last keyframe.

  These examples assume `use Emerge.UI`.

  ## Keyframes

  Every keyframe must use the same attribute set throughout the animation.
  Matching attrs must also stay in compatible variants.

  Examples:

  - `width(px(80))` can animate to `width(px(160))`, but not to `width(fill())`
  - `padding(8)` can animate to `padding(16)`, but not to `padding_each(8, 12, 8, 12)`
  - `Background.color(...)` can animate to another color background, and
    `Background.gradient(...)` can animate to another gradient background
  - image backgrounds must keep the same source and fit across keyframes
  - `Border.shadow/1` and `Border.glow/2` must keep the same shadow count in
    each keyframe

  ## Animate

  `animate/4` runs during normal retained updates. Use it for looping motion
  and other ongoing transitions.

      el(
        [
          Animation.animate(
            [
              [Transform.move_x(-60), Transform.alpha(0.4)],
              [Transform.move_x(60), Transform.alpha(1.0)],
              [Transform.move_x(-60), Transform.alpha(0.4)]
            ],
            3000,
            :linear,
            :loop
          ),
          Background.color(color(:teal, 400)),
          Border.rounded(12),
          padding(10)
        ],
        text("Looping")
      )

  ## Animate Enter

  `animate_enter/4` starts when an element first mounts. It does not start if
  it is added later to an already retained node.

  The enter spec is captured at mount time, so changing it later does not
  affect the currently running enter animation.

  If both `animate_enter/4` and `animate/4` are present, the regular animation
  waits for the enter animation to finish and then starts from zero progress.

      el(
        [
          Animation.animate_enter(
            [
              [Transform.move_y(20), Transform.alpha(0.0)],
              [Transform.move_y(0), Transform.alpha(1.0)]
            ],
            180,
            :ease_out
          )
        ],
        text("Entering")
      )

  ## Animate Exit

  `animate_exit/4` starts when an element is removed from the tree. Emerge
  keeps rendering the element until the exit animation finishes.

  `animate_exit/4` must use `:once` and is not allowed on the viewport root.

      el(
        [
          Animation.animate_exit(
            [
              [Transform.alpha(1.0)],
              [Transform.alpha(0.0)]
            ],
            120,
            :linear
          )
        ],
        text("Leaving")
      )

  ## Animatable Attrs

  Animation keyframes support these public attr families:

  - layout: `width/1`, `height/1`, `padding/1`, `padding_each/4`, `spacing/1`, `spacing_xy/2`
  - background: `Background.*`
  - border: `Border.rounded/1`, `Border.rounded_each/4`, `Border.width/1`,
    `Border.width_each/4`, `Border.color/1`, `Border.shadow/1`, `Border.glow/2`
  - font and svg color: `Font.size/1`, `Font.color/1`, `Font.letter_spacing/1`,
    `Font.word_spacing/1`, `Svg.color/1`
  - transforms: `Transform.move_x/1`, `Transform.move_y/1`,
    `Transform.rotate/1`, `Transform.scale/1`, `Transform.alpha/1`

  Other attrs, such as events, alignment, and font family/style/weight helpers,
  are not animatable.

  ## Layout

  Animation overlays are applied before measurement and layout each frame.

  That means animated layout attrs such as `width/1`, `height/1`, `padding/1`,
  and `spacing/1` participate in relayout. The first keyframe also establishes
  the initial layout state before any animation time has elapsed.
  """

  @type curve :: :linear | :ease_in | :ease_out | :ease_in_out
  @type repeat :: :once | :loop | pos_integer()
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

  @doc "Animate animatable attrs across keyframes during normal updates."
  @spec animate(keyframes(), number(), curve()) :: animate_attr()
  @spec animate(keyframes(), number(), curve(), repeat()) :: animate_attr()
  def animate(keyframes, duration, curve, repeat \\ :once) do
    {:animate, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc """
  Animate animatable attrs when the element first mounts.

  Unlike `animate/4`, this does not start if it is added later to an existing retained node.
  If both `animate_enter/4` and `animate/4` are present, `animate/4` starts after the
  enter animation completes.
  """
  @spec animate_enter(keyframes(), number(), curve()) :: animate_enter_attr()
  @spec animate_enter(keyframes(), number(), curve(), repeat()) :: animate_enter_attr()
  def animate_enter(keyframes, duration, curve, repeat \\ :once) do
    {:animate_enter, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end

  @doc """
  Animate animatable attrs once when the element is removed from the tree.

  The element keeps rendering until the exit animation completes.
  `animate_exit/4` is not allowed on the viewport root.
  """
  @spec animate_exit(keyframes(), number(), curve()) :: animate_exit_attr()
  @spec animate_exit(keyframes(), number(), curve(), :once) :: animate_exit_attr()
  def animate_exit(keyframes, duration, curve, repeat \\ :once) do
    {:animate_exit, %{keyframes: keyframes, duration: duration, curve: curve, repeat: repeat}}
  end
end
