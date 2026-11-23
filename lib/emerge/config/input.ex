defmodule Emerge.Config.Input do
  @moduledoc """
  Input observation, scroll distance and cursor settings. Set this record as
  `Emerge.Config.input`. Ordinary element interactions do not require subscriptions.

  ```elixir
  %Emerge.Config{
    input: %Emerge.Config.Input{events: [:key, :pointer_button, :resize]}
  }
  ```

  ## Fields

  - `events` — categories forwarded to `c:Emerge.handle_input/2`; defaults to
    `[]` (none). Choose `:key`, `:text`, `:pointer_move`, `:pointer_button`,
    `:pointer_scroll`, `:pointer_enter`, `:resize`, `:focus`, `:touch` and/or
    `:display_wake`. `all_events/0` returns all ten. Duplicates and order do not matter.
    Integers, `nil`, `:all` and `:none` are not accepted.
  - `scroll_line_pixels` — positive pixel distance per wheel/line-scroll step;
    defaults to `30.0`. Precise pixel-scroll input is not converted into line steps.
  - `scroll_bounce` — with `true`, a touch or pointer drag pulls scroll content past
    its edge with growing resistance, and the content springs back on release; a
    fling that reaches the edge overshoots and returns. A scroll container whose
    content fits bounces as well. Defaults to `false` (hard stop).
    Wheel, keyboard, scrollbar and app-set positions stay clamped. Supported by the
    DRM and Wayland backends.
  - `scroll_deceleration` — how a fling slows down after release. `:android`
    (default) follows Android's `OverScroller` curve. `:ios` follows
    `UIScrollView.DecelerationRate.normal`: the velocity drops by 0.2 % per
    millisecond, so a fling travels about half a second's worth of its release
    velocity.
  - `scrollbars` — with `:always` (default), scrollbar thumbs are visible whenever
    content overflows. With `:while_scrolling`, they are visible while a container
    scrolls (drag, fling, wheel, keys, overscroll) and fade out 800 ms after the last
    movement. In both modes the renderer draws thumbs over the content, without
    layout space.
  - `cursor` — `Emerge.Config.Cursor`, defaulting to its built-in cursor settings.

  Subscriptions filter input callbacks, not buttons, element key bindings,
  text editing, scrolling or `c:Emerge.handle_close/2`. The `:text` category includes
  committed text and IME updates. Enter/focus describe a window, not an element.
  See the [input guide](handle_input.html) and `Emerge.Input` for callback values
  and which events each backend can provide. DRM touch wake is configured with
  `Emerge.Config.DRM.wake_on_touch`. Include `:display_wake` in `events` to receive
  `{:display_wake, gesture}` through `handle_input/2`. This subscription observes
  accepted wakes; it does not enable waking. `:touch` alone does not subscribe to
  wake notifications.

  ## Changing settings

  `events` can change through `config` and replaces the entire subscription list.
  Messages already in your mailbox are not retracted. Scroll distance, scroll bounce,
  scroll deceleration and cursor settings require a new viewport to change.
  """
  defstruct events: [],
            scroll_line_pixels: 30.0,
            scroll_bounce: false,
            scroll_deceleration: :android,
            scrollbars: :always,
            cursor: %Emerge.Config.Cursor{}

  @typedoc "Input category. `:text` includes committed text and IME events; `:display_wake` observes accepted DRM wakes; enter/focus refer to the window, not an element."
  @type event_kind ::
          :key
          | :text
          | :pointer_move
          | :pointer_button
          | :pointer_scroll
          | :pointer_enter
          | :resize
          | :focus
          | :touch
          | :display_wake
  @type t :: %__MODULE__{
          events: [event_kind()],
          scroll_line_pixels: number(),
          scroll_bounce: boolean(),
          scroll_deceleration: :android | :ios,
          scrollbars: :always | :while_scrolling,
          cursor: Emerge.Config.Cursor.t() | Emerge.Config.Cursor.options()
        }
  @typedoc "Typed keyword shorthand accepted in this record's parent field."
  @type options :: [
          {:events, [event_kind()]}
          | {:scroll_line_pixels, number()}
          | {:scroll_bounce, boolean()}
          | {:scroll_deceleration, :android | :ios}
          | {:scrollbars, :always | :while_scrolling}
          | {:cursor, Emerge.Config.Cursor.t() | Emerge.Config.Cursor.options()}
        ]
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          events: [event_kind()],
          scroll_line_pixels: number(),
          scroll_bounce: boolean(),
          scroll_deceleration: :android | :ios,
          scrollbars: :always | :while_scrolling,
          cursor: Emerge.Config.Cursor.normalized_t()
        }

  @doc false
  def __schema__,
    do: [
      events:
        {:set,
         {:enum,
          [
            :key,
            :text,
            :pointer_move,
            :pointer_button,
            :pointer_scroll,
            :pointer_enter,
            :resize,
            :focus,
            :touch,
            :display_wake
          ]}},
      scroll_line_pixels: :positive_number,
      scroll_bounce: :boolean,
      scroll_deceleration: {:enum, [:android, :ios]},
      scrollbars: {:enum, [:always, :while_scrolling]},
      cursor: {:record, Emerge.Config.Cursor}
    ]

  @doc "Returns every supported input category."
  @spec all_events() :: [event_kind()]
  def all_events,
    do: [
      :key,
      :text,
      :pointer_move,
      :pointer_button,
      :pointer_scroll,
      :pointer_enter,
      :resize,
      :focus,
      :touch,
      :display_wake
    ]
end
