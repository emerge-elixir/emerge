defmodule Emerge.Input do
  @moduledoc """
  Raw input values passed as the first argument to `c:Emerge.handle_input/2`.

  Subscribe using `Emerge.Config.Input.events`. These observers are independent
  of element events (`handle_info/2`); disabling them does not disable UI input.

  | Subscription | Callback argument |
  |---|---|
  | `:key` | `{:key, {key, :press \| :release, modifiers}}` |
  | `:text` | `{:text_commit, {text, modifiers}}`, `{:text_preedit, {text, cursor_range_or_nil}}`, `:text_preedit_clear`, `{:delete_surrounding, {before_bytes, after_bytes}}` |
  | `:pointer_move` | `{:pointer_move, {x, y}}` |
  | `:pointer_button` | `{:pointer_button, {button, :press \| :release, modifiers, {x, y}}}` |
  | `:pointer_scroll` | `{:pointer_scroll, {{dx, dy}, {x, y}}}` |
  | `:pointer_enter` | `{:pointer_enter, entered?}` (window enter/leave, not element hover) |
  | `:resize` | `{:resize, {width, height, scale}}` |
  | `:focus` | `{:focus, focused?}` (window focus, not element focus) |
  | `:touch` | `{:touch, {contact_id, :down \| :move \| :up \| :cancel, {x, y}}}` |

  Keys identify canonical keys, not inserted characters; use `:text` for text
  input and IME composition. Repeated key presses are delivered as `:press`.
  Committed text may contain multiple Unicode characters. Surrounding deletion
  lengths are UTF-8 bytes. A preedit cursor range is optional and is supplied by
  the platform's IME, not a selection in the application's committed text.

  Pointer positions are backend surface coordinates, not the logical positions
  delivered by `Emerge.UI.Event` mouse or drag handlers. Use element events for
  control-local coordinates and captured dragging; raw observers do not capture
  a control or change native dispatch.

  Discrete scroll steps
  are normalized using `scroll_line_pixels`; precise scrolling uses backend
  deltas. Positive deltas follow the backend's normalized scroll direction.
  Resize includes output dimensions and the backend scale factor.

  Event availability depends on the backend: DRM has no desktop window
  focus/enter lifecycle; headless has no physical input source. IME operations
  depend on platform support. Subscribing does not synthesize missing events.
  Window-close requests go to `handle_close/2`, never this callback.

  DRM touchscreens deliver contact-local `:touch` events, not mouse button
  emulation to raw observers. Contact IDs identify one lifetime; reused hardware
  slots receive new IDs. Up/cancel carry that contact's last position. A cancel
  never activates a virtual key. Mouse subscriptions remain unchanged.

      def handle_input({:key, {:escape, :press, _modifiers}}, state) do
        {:noreply, state}
      end

      def handle_input(_event, state), do: {:noreply, state}
  """

  @type action :: :press | :release
  @type modifier :: :shift | :ctrl | :alt | :meta
  @type button :: :left | :right | :middle | :back | :forward | :other
  @type position :: {number(), number()}
  @type cursor_range :: {non_neg_integer(), non_neg_integer()} | nil
  @typedoc "Canonical key identifiers shared by all backends."
  @type key ::
          :a
          | :b
          | :c
          | :d
          | :e
          | :f
          | :g
          | :h
          | :i
          | :j
          | :k
          | :l
          | :m
          | :n
          | :o
          | :p
          | :q
          | :r
          | :s
          | :t
          | :u
          | :v
          | :w
          | :x
          | :y
          | :z
          | :digit_0
          | :digit_1
          | :digit_2
          | :digit_3
          | :digit_4
          | :digit_5
          | :digit_6
          | :digit_7
          | :digit_8
          | :digit_9
          | :minus
          | :equal
          | :plus
          | :asterisk
          | :left_bracket
          | :right_bracket
          | :backslash
          | :semicolon
          | :apostrophe
          | :grave
          | :comma
          | :period
          | :slash
          | :space
          | :enter
          | :tab
          | :escape
          | :backspace
          | :insert
          | :delete
          | :home
          | :end
          | :page_up
          | :page_down
          | :arrow_left
          | :arrow_right
          | :arrow_up
          | :arrow_down
          | :shift
          | :control
          | :alt
          | :alt_graph
          | :super
          | :caps_lock
          | :num_lock
          | :scroll_lock
          | :print_screen
          | :pause
          | :context_menu
          | :f1
          | :f2
          | :f3
          | :f4
          | :f5
          | :f6
          | :f7
          | :f8
          | :f9
          | :f10
          | :f11
          | :f12
          | :f13
          | :f14
          | :f15
          | :f16
          | :f17
          | :f18
          | :f19
          | :f20
          | :f21
          | :f22
          | :f23
          | :f24
          | :unknown
  @type key_event :: {:key, {key(), action(), [modifier()]}}
  @type text_event ::
          {:text_commit, {String.t(), [modifier()]}}
          | {:text_preedit, {String.t(), cursor_range()}}
          | :text_preedit_clear
          | {:delete_surrounding, {non_neg_integer(), non_neg_integer()}}
  @type pointer_event ::
          {:pointer_move, position()}
          | {:pointer_button, {button(), action(), [modifier()], position()}}
          | {:pointer_scroll, {position(), position()}}
          | {:pointer_enter, boolean()}
  @type event ::
          key_event()
          | text_event()
          | pointer_event()
          | {:touch, {non_neg_integer(), :down | :move | :up | :cancel, position()}}
          | {:resize, {non_neg_integer(), non_neg_integer(), number()}}
          | {:focus, boolean()}

  @doc false
  def from_native({:key, {key, action, mods}}), do: {:key, {key, action(action), mods}}

  def from_native({:cursor_button, {button, action, mods, pos}}),
    do: {:pointer_button, {button, action(action), mods, pos}}

  def from_native({:cursor_pos, pos}), do: {:pointer_move, pos}
  def from_native({:cursor_scroll, scroll}), do: {:pointer_scroll, scroll}
  def from_native({:cursor_entered, entered}), do: {:pointer_enter, entered}
  def from_native({:resized, dimensions}), do: {:resize, dimensions}
  def from_native({:focused, focused}), do: {:focus, focused}
  def from_native(event), do: event

  defp action(0), do: :release
  defp action(1), do: :press
end
