defmodule Emerge.UI.Event do
  @moduledoc """
  Event handler helpers for interactive elements.

  Event helpers attach process messages to elements. When the event fires,
  Emerge sends the stored message to the target process.

  All helpers accept either:

  - a bare `message`
  - a `{pid, message}` tuple

  Passing only a `message` is shorthand for `{self(), message}`. Inside a
  viewport `render/0` or `render/1` callback, that means the viewport process, so the
  message arrives in `handle_info/2`.

  ## Payload Routing

  Some element events include a payload. Today the main public example is
  `on_change/1` for text-input value changes.

  When a payload-bearing event is delivered through a viewport, the payload is
  wrapped into the stored message using the viewport's `wrap_payload/3`
  callback. The default behavior is:

  - non-tuple message: `{message, payload}`
  - tuple message: append the payload to the tuple

  For example:

  This field sends the new value back to the viewport whenever the text changes.

  ```elixir
  def render(state) do
    Input.text(
      [key(:search), Event.on_change(:search_changed)],
      state.query
    )
  end

  def handle_info({:search_changed, value}, state) do
    {:noreply, %{state | query: value} |> Viewport.rerender()}
  end
  ```

  If you want more structure in the delivered message, store a tuple message:

  This is useful when several fields share the same `handle_info/2` path and you
  want the message to identify which field changed.

  ```elixir
  Input.text([Event.on_change({self(), {:search_changed, :field}})], state.query)

  def handle_info({:search_changed, :field, value}, state) do
    {:noreply, %{state | query: value} |> Viewport.rerender()}
  end
  ```

  ## Pointer Events

  Prefer `on_press/1` for normal button-like activation. It is the default for
  actions such as save, submit, open, and delete.

  Use `on_click/1` when you specifically want pointer click behavior.

  Swipe helpers emit once a pointer gesture resolves to that direction.

  `on_mouse_down/1` and `on_mouse_up/1` are left-button only.

  `on_mouse_enter/1`, `on_mouse_leave/1`, and `on_mouse_move/1` do not include
  cursor coordinates in the delivered message.

  Events do not bubble to parent elements. Attach the handler to the element
  that should react.

  If you only want visual hover, focus, or pressed styling, use
  `Emerge.UI.Interactive` instead of event handlers.

  ## Input And Focus

  `on_change/1` is intended for text inputs.

  Text editing still works without `on_change/1`; the handler only controls
  whether a change message is emitted.

  `on_focus/1` and `on_blur/1` work on focusable elements, not only text inputs.

  ## Keyboard Events

  Focused elements can listen for keyboard input with:

  - `on_key_down/2`
  - `on_key_up/2`
  - `on_key_press/2`

  Keyboard matchers can be written as either:

  - a key atom such as `:enter`
  - a keyword matcher such as `[key: :digit_1, mods: [:ctrl], match: :all]`

  Supported modifier atoms are `:shift`, `:ctrl`, `:alt`, and `:meta`.

  Match modes are:

  - `:exact` - the modifier set must match exactly
  - `:all` - the listed modifiers must be present, but extra modifiers are ok

  `on_key_press/2` is for completed key gestures. It fires on release after a
  matching press.

  You can attach multiple keyboard listeners to one element by listing
  `on_key_down/2`, `on_key_up/2`, or `on_key_press/2` more than once.

  ## Virtual Keys

  `virtual_key/1` is for on-screen keyboard keys and similar soft-key controls.

  A virtual key spec must include `:tap`, which can be one of:

  - `{:text, binary}`
  - `{:key, key, mods}`
  - `{:text_and_key, text, key, mods}`

  Optional hold behavior is:

  - `nil` - no hold behavior
  - `:repeat` - repeat the tap action while held
  - `{:event, payload}` - emit a separate hold event

  Defaults:

  - `hold_ms: 350`
  - `repeat_ms: 40`

  `virtual_key/1` cannot be combined with `on_click/1` or `on_press/1` on the
  same element.

  ## Examples

  In this example, the first button sends `:save`, the second reacts to focused
  `Enter`, and the third acts like a soft keyboard key.

  ```elixir
  def render(_state) do
    column([spacing(16)], [
      Input.button(
        [
          Event.on_press(:save),
          Background.color(color(:sky, 500)),
          Border.rounded(8),
          Font.color(color(:white)),
          padding(12)
        ],
        text("Save")
      ),
      Input.button(
        [Event.on_key_down(:enter, :submit)],
        text("Submit")
      ),
      Input.button(
        [Event.virtual_key(tap: {:text_and_key, "A", :a, [:shift]})],
        text("A")
      )
    ])
  end
  ```
  """

  @typedoc """
  Destination process and message sent when the event fires.

  Most helpers also accept a bare `message`, which is shorthand for
  `{self(), message}`.
  """
  @type payload :: {pid(), term()}
  @type click_attr :: {:on_click, payload()}
  @type press_attr :: {:on_press, payload()}
  @type swipe_up_attr :: {:on_swipe_up, payload()}
  @type swipe_down_attr :: {:on_swipe_down, payload()}
  @type swipe_left_attr :: {:on_swipe_left, payload()}
  @type swipe_right_attr :: {:on_swipe_right, payload()}
  @type mouse_down_attr :: {:on_mouse_down, payload()}
  @type mouse_up_attr :: {:on_mouse_up, payload()}
  @type mouse_enter_attr :: {:on_mouse_enter, payload()}
  @type mouse_leave_attr :: {:on_mouse_leave, payload()}
  @type mouse_move_attr :: {:on_mouse_move, payload()}
  @type change_attr :: {:on_change, payload()}
  @type focus_attr :: {:on_focus, payload()}
  @type blur_attr :: {:on_blur, payload()}

  @typedoc "Modifier keys accepted by keyboard matchers."
  @type key_modifier :: :shift | :ctrl | :alt | :meta

  @typedoc """
  How modifier matching is interpreted for keyboard listeners.

  - `:exact` requires the active modifiers to match exactly.
  - `:all` requires the listed modifiers to be present, but allows extras.
  """
  @type key_match_mode :: :exact | :all

  @typedoc "Canonical key atoms accepted by keyboard event helpers."
  @type key_name ::
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

  @typedoc """
  Keyboard matcher accepted by `on_key_down/2`, `on_key_up/2`, and
  `on_key_press/2`.

  Use either a key atom like `:enter` or a keyword matcher like
  `[key: :digit_1, mods: [:ctrl], match: :all]`.
  """
  @type key_matcher ::
          key_name()
          | [key: key_name(), mods: [key_modifier()], match: key_match_mode()]

  @typedoc """
  Normalized keyboard binding stored on an element.

  `route` is derived automatically from the key, modifiers, and match mode.
  """
  @type key_binding :: %{
          required(:key) => key_name(),
          required(:mods) => [key_modifier()],
          required(:match) => key_match_mode(),
          required(:payload) => payload(),
          required(:route) => binary()
        }

  @typedoc "Public descriptor form of a normalized keyboard binding."
  @type key_binding_descriptor :: %{
          required(:key) => key_name(),
          required(:mods) => [key_modifier()],
          required(:match) => key_match_mode(),
          required(:route) => binary()
        }

  @typedoc "Tap behavior for `virtual_key/1`."
  @type virtual_key_tap ::
          {:text, binary()}
          | {:key, key_name(), [key_modifier()]}
          | {:text_and_key, binary(), key_name(), [key_modifier()]}

  @typedoc "Optional hold behavior for `virtual_key/1`."
  @type virtual_key_hold :: nil | :repeat | {:event, payload()}

  @typedoc """
  User-facing virtual key spec.

  Required key:

  - `:tap`

  Optional keys:

  - `:hold`
  - `:hold_ms`
  - `:repeat_ms`
  """
  @type virtual_key_spec :: %{
          required(:tap) => virtual_key_tap(),
          optional(:hold) => virtual_key_hold(),
          optional(:hold_ms) => non_neg_integer(),
          optional(:repeat_ms) => pos_integer()
        }

  @typedoc "Normalized descriptor form of a virtual key spec."
  @type virtual_key_descriptor :: %{
          required(:tap) => virtual_key_tap(),
          required(:hold) => :none | :repeat | :event,
          required(:hold_ms) => non_neg_integer(),
          required(:repeat_ms) => pos_integer()
        }

  @type key_down_attr :: {:on_key_down, key_binding()}
  @type key_up_attr :: {:on_key_up, key_binding()}
  @type key_press_attr :: {:on_key_press, key_binding()}
  @type virtual_key_attr :: {:virtual_key, virtual_key_spec()}

  @type t ::
          click_attr()
          | press_attr()
          | swipe_up_attr()
          | swipe_down_attr()
          | swipe_left_attr()
          | swipe_right_attr()
          | mouse_down_attr()
          | mouse_up_attr()
          | mouse_enter_attr()
          | mouse_leave_attr()
          | mouse_move_attr()
          | change_attr()
          | focus_attr()
          | blur_attr()
          | key_down_attr()
          | key_up_attr()
          | key_press_attr()
          | virtual_key_attr()

  @key_modifiers [:shift, :ctrl, :alt, :meta]
  @default_virtual_key_hold_ms 350
  @default_virtual_key_repeat_ms 40

  @key_names [
    :a,
    :b,
    :c,
    :d,
    :e,
    :f,
    :g,
    :h,
    :i,
    :j,
    :k,
    :l,
    :m,
    :n,
    :o,
    :p,
    :q,
    :r,
    :s,
    :t,
    :u,
    :v,
    :w,
    :x,
    :y,
    :z,
    :digit_0,
    :digit_1,
    :digit_2,
    :digit_3,
    :digit_4,
    :digit_5,
    :digit_6,
    :digit_7,
    :digit_8,
    :digit_9,
    :minus,
    :equal,
    :plus,
    :asterisk,
    :left_bracket,
    :right_bracket,
    :backslash,
    :semicolon,
    :apostrophe,
    :grave,
    :comma,
    :period,
    :slash,
    :space,
    :enter,
    :tab,
    :escape,
    :backspace,
    :insert,
    :delete,
    :home,
    :end,
    :page_up,
    :page_down,
    :arrow_left,
    :arrow_right,
    :arrow_up,
    :arrow_down,
    :shift,
    :control,
    :alt,
    :alt_graph,
    :super,
    :caps_lock,
    :num_lock,
    :scroll_lock,
    :print_screen,
    :pause,
    :context_menu,
    :f1,
    :f2,
    :f3,
    :f4,
    :f5,
    :f6,
    :f7,
    :f8,
    :f9,
    :f10,
    :f11,
    :f12,
    :f13,
    :f14,
    :f15,
    :f16,
    :f17,
    :f18,
    :f19,
    :f20,
    :f21,
    :f22,
    :f23,
    :f24,
    :unknown
  ]

  @key_name_set MapSet.new(@key_names)
  @modifier_mask %{shift: 0x01, ctrl: 0x02, alt: 0x04, meta: 0x08}

  @doc """
  Register a pointer click payload for this element.

  Use this when you specifically want click behavior from the pointer.

  For normal button-like activation, prefer `on_press/1`.
  """
  @spec on_click(payload() | term()) :: click_attr()
  def on_click({pid, _msg} = payload) when is_pid(pid), do: {:on_click, payload}
  def on_click(message), do: on_click({self(), message})

  @doc """
  Register a press payload for this element.

  This is the recommended default for buttons and other action controls.
  For standard activation, prefer `on_press/1` over `on_click/1` because it
  also works with focused `Enter`.

  ## Examples

  This is a conventional action button: pressing it sends `:save` to the target
  process while the surrounding attrs provide the visual styling.

  ```elixir
  Input.button(
    [
      Event.on_press(:save),
      Background.color(color(:sky, 500)),
      Border.rounded(8),
      Font.color(color(:white)),
      padding(12)
    ],
    text("Save")
  )
  ```
  """
  @spec on_press(payload() | term()) :: press_attr()
  def on_press({pid, _msg} = payload) when is_pid(pid), do: {:on_press, payload}
  def on_press(message), do: on_press({self(), message})

  @doc "Register a swipe-up payload for this element. Fires when the gesture resolves upward."
  @spec on_swipe_up(payload() | term()) :: swipe_up_attr()
  def on_swipe_up({pid, _msg} = payload) when is_pid(pid), do: {:on_swipe_up, payload}
  def on_swipe_up(message), do: on_swipe_up({self(), message})

  @doc "Register a swipe-down payload for this element. Fires when the gesture resolves downward."
  @spec on_swipe_down(payload() | term()) :: swipe_down_attr()
  def on_swipe_down({pid, _msg} = payload) when is_pid(pid), do: {:on_swipe_down, payload}
  def on_swipe_down(message), do: on_swipe_down({self(), message})

  @doc "Register a swipe-left payload for this element. Fires when the gesture resolves leftward."
  @spec on_swipe_left(payload() | term()) :: swipe_left_attr()
  def on_swipe_left({pid, _msg} = payload) when is_pid(pid), do: {:on_swipe_left, payload}
  def on_swipe_left(message), do: on_swipe_left({self(), message})

  @doc "Register a swipe-right payload for this element. Fires when the gesture resolves rightward."
  @spec on_swipe_right(payload() | term()) :: swipe_right_attr()
  def on_swipe_right({pid, _msg} = payload) when is_pid(pid), do: {:on_swipe_right, payload}
  def on_swipe_right(message), do: on_swipe_right({self(), message})

  @doc "Register a mouse-down payload for this element. Left mouse button only."
  @spec on_mouse_down(payload() | term()) :: mouse_down_attr()
  def on_mouse_down({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_down, payload}
  def on_mouse_down(message), do: on_mouse_down({self(), message})

  @doc "Register a mouse-up payload for this element. Left mouse button only."
  @spec on_mouse_up(payload() | term()) :: mouse_up_attr()
  def on_mouse_up({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_up, payload}
  def on_mouse_up(message), do: on_mouse_up({self(), message})

  @doc "Register a mouse-enter payload for this element. Delivered without cursor coordinates."
  @spec on_mouse_enter(payload() | term()) :: mouse_enter_attr()
  def on_mouse_enter({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_enter, payload}
  def on_mouse_enter(message), do: on_mouse_enter({self(), message})

  @doc "Register a mouse-leave payload for this element. Delivered without cursor coordinates."
  @spec on_mouse_leave(payload() | term()) :: mouse_leave_attr()
  def on_mouse_leave({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_leave, payload}
  def on_mouse_leave(message), do: on_mouse_leave({self(), message})

  @doc "Register a mouse-move payload for this element. Delivered without cursor coordinates."
  @spec on_mouse_move(payload() | term()) :: mouse_move_attr()
  def on_mouse_move({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_move, payload}
  def on_mouse_move(message), do: on_mouse_move({self(), message})

  @doc """
  Register a value-change payload for a text input.

  Use `on_change/1` with `Emerge.UI.Input.text/2` or
  `Emerge.UI.Input.multiline/2`.

  Text editing still works without this handler; `on_change/1` only controls
  whether a message is emitted when the value changes.

  When delivered through a viewport, the changed value is wrapped into the
  stored message using `wrap_payload/3`. Multiline values use the same payload
  shape; newline characters remain part of the emitted binary.

  ## Examples

  This example keeps the input value in viewport state and updates that state
  whenever the field changes.

  ```elixir
  def render(state) do
    Input.text(
      [
        key(:search),
        Event.on_change(:search_changed)
      ],
      state.query
    )
  end

  def handle_info({:search_changed, value}, state) do
    {:noreply, %{state | query: value} |> Viewport.rerender()}
  end
  ```
  """
  @spec on_change(payload() | term()) :: change_attr()
  def on_change({pid, _msg} = payload) when is_pid(pid), do: {:on_change, payload}
  def on_change(message), do: on_change({self(), message})

  @doc "Register a focus payload for a focusable element."
  @spec on_focus(payload() | term()) :: focus_attr()
  def on_focus({pid, _msg} = payload) when is_pid(pid), do: {:on_focus, payload}
  def on_focus(message), do: on_focus({self(), message})

  @doc "Register a blur payload for a focusable element."
  @spec on_blur(payload() | term()) :: blur_attr()
  def on_blur({pid, _msg} = payload) when is_pid(pid), do: {:on_blur, payload}
  def on_blur(message), do: on_blur({self(), message})

  @doc """
  Register a focused key-down payload for this element.

  `matcher` can be a key atom like `:enter` or a keyword matcher such as
  `[key: :digit_1, mods: [:ctrl], match: :all]`.

  `:match` defaults to `:exact`.

  ## Example

  This lets one focused button respond both to `Ctrl+1` and to plain `Enter`.

  ```elixir
  Input.button(
    [
      Event.on_key_down([key: :digit_1, mods: [:ctrl], match: :all], :select_tab_1),
      Event.on_key_down(:enter, :submit)
    ],
    text("Save")
  )
  ```

  On focused `Emerge.UI.Input.text/2` and `Emerge.UI.Input.multiline/2`
  elements, a matching `on_key_down/2` suppresses the input's default keydown
  behavior for that keydown. This lets apps override built-in editing such as
  character insertion or multiline `Enter` handling.

  For example, `on_key_down(:enter, :submit)` on a focused
  `Emerge.UI.Input.multiline/2` intercepts `Enter` before the default newline is
  inserted.
  """
  @spec on_key_down(key_matcher(), payload() | term()) :: key_down_attr()
  def on_key_down(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_down, build_key_binding!(:key_down, matcher, payload)}
  end

  def on_key_down(matcher, message), do: on_key_down(matcher, {self(), message})

  @doc """
  Register a focused key-up payload for this element.

  Key-up handlers use the same matcher forms as `on_key_down/2`.
  """
  @spec on_key_up(key_matcher(), payload() | term()) :: key_up_attr()
  def on_key_up(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_up, build_key_binding!(:key_up, matcher, payload)}
  end

  def on_key_up(matcher, message), do: on_key_up(matcher, {self(), message})

  @doc """
  Register a focused completed key-press payload for this element.

  `on_key_press/2` is for completed key gestures and fires on release after a
  matching press.
  """
  @spec on_key_press(key_matcher(), payload() | term()) :: key_press_attr()
  def on_key_press(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_press, build_key_binding!(:key_press, matcher, payload)}
  end

  def on_key_press(matcher, message), do: on_key_press(matcher, {self(), message})

  @doc """
  Register virtual-key behavior for an element.

  This is useful for on-screen keyboards and similar soft-key controls.

  The spec must include `:tap`. Optional keys are `:hold`, `:hold_ms`, and
  `:repeat_ms`.

  `virtual_key/1` cannot be combined with `on_click/1` or `on_press/1` on the
  same element.

  ## Example

  This virtual key inserts an uppercase `A` on tap and sends a separate
  `:show_alternates` event when the key is held.

  ```elixir
  Input.button(
    [
      width(px(56)),
      height(px(56)),
      Event.virtual_key(
        tap: {:text_and_key, "A", :a, [:shift]},
        hold: {:event, {self(), :show_alternates}}
      )
    ],
    text("A")
  )
  ```

  `{:text_and_key, text, key, mods}` participates in the same text-input
  keydown suppression rules as a physical key press. `{:text, text}` does not,
  because it inserts text without a preceding keydown. See `on_key_down/2` for
  the suppression behavior on focused `Input.text/2` and `Input.multiline/2`.
  """
  @spec virtual_key(virtual_key_spec() | keyword()) :: virtual_key_attr()
  def virtual_key(spec) do
    {:virtual_key, normalize_virtual_key!(spec)}
  end

  @doc false
  @spec normalize_key_listener_bindings!(:on_key_down | :on_key_up | :on_key_press, term()) ::
          [key_binding()]
  def normalize_key_listener_bindings!(attr, value)
      when attr in [:on_key_down, :on_key_up, :on_key_press] do
    case value do
      %{} = binding ->
        [normalize_key_binding!(attr, binding)]

      bindings when is_list(bindings) ->
        Enum.map(bindings, &normalize_key_binding!(attr, &1))

      other ->
        raise ArgumentError,
              "#{inspect(attr)} expects a key binding map or list of key binding maps, got: #{inspect(other)}"
    end
  end

  @doc false
  @spec normalize_virtual_key!(virtual_key_spec() | keyword()) :: virtual_key_spec()
  def normalize_virtual_key!(value) do
    spec = normalize_virtual_key_spec_map!(value)
    ensure_only_keys!(:virtual_key, spec, [:tap, :hold, :hold_ms, :repeat_ms])

    tap =
      case Map.fetch(spec, :tap) do
        {:ok, tap} -> normalize_virtual_key_tap!(tap)
        :error -> missing_virtual_key_tap!()
      end

    hold =
      spec
      |> Map.get(:hold, nil)
      |> normalize_virtual_key_hold!()

    hold_ms =
      normalize_virtual_key_hold_ms!(Map.get(spec, :hold_ms, @default_virtual_key_hold_ms))

    repeat_ms =
      normalize_virtual_key_repeat_ms!(Map.get(spec, :repeat_ms, @default_virtual_key_repeat_ms))

    %{tap: tap, hold: hold, hold_ms: hold_ms, repeat_ms: repeat_ms}
  end

  @doc false
  @spec virtual_key_descriptor(virtual_key_spec()) :: virtual_key_descriptor()
  def virtual_key_descriptor(spec) do
    spec = normalize_virtual_key_spec_map!(spec)
    ensure_only_keys!(:virtual_key, spec, [:tap, :hold, :hold_ms, :repeat_ms])

    tap =
      spec
      |> Map.fetch!(:tap)
      |> normalize_virtual_key_tap!()

    hold =
      spec
      |> Map.get(:hold, nil)
      |> normalize_virtual_key_hold_descriptor!()

    hold_ms =
      normalize_virtual_key_hold_ms!(Map.get(spec, :hold_ms, @default_virtual_key_hold_ms))

    repeat_ms =
      normalize_virtual_key_repeat_ms!(Map.get(spec, :repeat_ms, @default_virtual_key_repeat_ms))

    %{tap: tap, hold: hold, hold_ms: hold_ms, repeat_ms: repeat_ms}
  end

  @doc false
  @spec key_route_id(
          :key_down | :key_up | :key_press,
          key_name(),
          [key_modifier()],
          key_match_mode()
        ) ::
          binary()
  def key_route_id(event_type, key, mods, match)
      when event_type in [:key_down, :key_up, :key_press] and is_atom(key) and is_list(mods) and
             match in [:exact, :all] do
    mods_mask =
      Enum.reduce(mods, 0, fn mod, acc -> Bitwise.bor(acc, Map.fetch!(@modifier_mask, mod)) end)

    "#{event_type}:#{key}:#{match}:#{mods_mask}"
  end

  @doc false
  @spec key_binding_descriptor(key_binding() | key_binding_descriptor()) ::
          key_binding_descriptor()
  def key_binding_descriptor(%{key: key, mods: mods, match: match, route: route}) do
    %{key: key, mods: mods, match: match, route: route}
  end

  defp normalize_virtual_key_spec_map!(value) when is_map(value), do: value

  defp normalize_virtual_key_spec_map!(value) when is_list(value) do
    if Keyword.keyword?(value) do
      Map.new(value)
    else
      raise ArgumentError,
            "virtual_key expects a map or keyword spec, got: #{inspect(value)}"
    end
  end

  defp normalize_virtual_key_spec_map!(value) do
    raise ArgumentError,
          "virtual_key expects a map or keyword spec, got: #{inspect(value)}"
  end

  defp normalize_virtual_key_tap!({:text, text}) when is_binary(text), do: {:text, text}

  defp normalize_virtual_key_tap!({:key, key, mods}) do
    {:key, normalize_key_name!(key, :virtual_key), normalize_key_modifiers!(mods, :virtual_key)}
  end

  defp normalize_virtual_key_tap!({:text_and_key, text, key, mods}) when is_binary(text) do
    {:text_and_key, text, normalize_key_name!(key, :virtual_key),
     normalize_key_modifiers!(mods, :virtual_key)}
  end

  defp normalize_virtual_key_tap!(other) do
    raise ArgumentError,
          "virtual_key expects :tap to be {:text, binary}, {:key, key, mods}, or {:text_and_key, text, key, mods}, got: #{inspect(other)}"
  end

  defp normalize_virtual_key_hold!(nil), do: nil
  defp normalize_virtual_key_hold!(:repeat), do: :repeat

  defp normalize_virtual_key_hold!({:event, payload}) do
    {:event, normalize_event_payload!(payload, :virtual_key)}
  end

  defp normalize_virtual_key_hold!(other) do
    raise ArgumentError,
          "virtual_key expects :hold to be nil, :repeat, or {:event, {pid, message}}, got: #{inspect(other)}"
  end

  defp normalize_virtual_key_hold_descriptor!(nil), do: :none
  defp normalize_virtual_key_hold_descriptor!(:none), do: :none
  defp normalize_virtual_key_hold_descriptor!(:repeat), do: :repeat

  defp normalize_virtual_key_hold_descriptor!({:event, payload}) do
    _ = normalize_event_payload!(payload, :virtual_key)
    :event
  end

  defp normalize_virtual_key_hold_descriptor!(:event), do: :event

  defp normalize_virtual_key_hold_descriptor!(other) do
    raise ArgumentError,
          "virtual_key expects :hold to be nil, :none, :repeat, :event, or {:event, {pid, message}}, got: #{inspect(other)}"
  end

  defp normalize_virtual_key_hold_ms!(value) when is_integer(value) and value >= 0, do: value

  defp normalize_virtual_key_hold_ms!(value) do
    raise ArgumentError,
          "virtual_key expects :hold_ms to be a non-negative integer, got: #{inspect(value)}"
  end

  defp normalize_virtual_key_repeat_ms!(value) when is_integer(value) and value > 0, do: value

  defp normalize_virtual_key_repeat_ms!(value) do
    raise ArgumentError,
          "virtual_key expects :repeat_ms to be a positive integer, got: #{inspect(value)}"
  end

  defp build_key_binding!(event_type, matcher, payload) do
    %{key: key, mods: mods, match: match} = normalize_key_matcher!(event_type, matcher)
    route = key_route_id(event_type, key, mods, match)
    %{key: key, mods: mods, match: match, payload: payload, route: route}
  end

  defp normalize_key_binding!(attr, %{} = binding) do
    ensure_only_keys!(attr, binding, [:key, :mods, :match, :payload, :route])

    key =
      case Map.fetch(binding, :key) do
        {:ok, key} -> normalize_key_name!(key, attr)
        :error -> missing_key_binding_field!(attr)
      end

    mods =
      binding
      |> Map.get(:mods, [])
      |> normalize_key_modifiers!(attr)

    match =
      binding
      |> Map.get(:match, :exact)
      |> normalize_key_match_mode!(attr)

    payload =
      case Map.fetch(binding, :payload) do
        {:ok, payload} -> normalize_event_payload!(payload, attr)
        :error -> missing_key_binding_field!(attr)
      end

    route = key_route_id(binding_event_type(attr), key, mods, match)
    %{key: key, mods: mods, match: match, payload: payload, route: route}
  end

  defp normalize_key_binding!(attr, other) do
    raise ArgumentError,
          "#{inspect(attr)} expects each key binding to be a map, got: #{inspect(other)}"
  end

  defp normalize_key_matcher!(event_type, matcher)
       when event_type in [:key_down, :key_up, :key_press] do
    case matcher do
      key when is_atom(key) ->
        %{key: normalize_key_name!(key, event_type), mods: [], match: :exact}

      keyword when is_list(keyword) ->
        if !Keyword.keyword?(keyword) do
          raise ArgumentError,
                "#{inspect(event_type)} expects a key atom or keyword matcher, got: #{inspect(keyword)}"
        end

        ensure_only_keys!(event_type, Map.new(keyword), [:key, :mods, :match])

        key =
          case Keyword.fetch(keyword, :key) do
            {:ok, key} -> normalize_key_name!(key, event_type)
            :error -> missing_key_matcher_key!(event_type)
          end

        mods =
          keyword
          |> Keyword.get(:mods, [])
          |> normalize_key_modifiers!(event_type)

        match =
          keyword
          |> Keyword.get(:match, :exact)
          |> normalize_key_match_mode!(event_type)

        %{key: key, mods: mods, match: match}

      other ->
        raise ArgumentError,
              "#{inspect(event_type)} expects a key atom or keyword matcher, got: #{inspect(other)}"
    end
  end

  defp normalize_key_name!(value, owner) when is_atom(value) do
    if MapSet.member?(@key_name_set, value) do
      value
    else
      raise ArgumentError,
            "#{inspect(owner)} expects a supported key atom, got: #{inspect(value)}"
    end
  end

  defp normalize_key_name!(value, owner) do
    raise ArgumentError,
          "#{inspect(owner)} expects a supported key atom, got: #{inspect(value)}"
  end

  defp normalize_key_modifiers!(value, owner) when is_list(value) do
    unique_mods = Enum.uniq(value)

    Enum.each(unique_mods, fn mod ->
      if mod not in @key_modifiers do
        raise ArgumentError,
              "#{inspect(owner)} expects modifiers to be drawn from #{inspect(@key_modifiers)}, got: #{inspect(mod)}"
      end
    end)

    Enum.filter(@key_modifiers, &(&1 in unique_mods))
  end

  defp normalize_key_modifiers!(value, owner) do
    raise ArgumentError,
          "#{inspect(owner)} expects :mods to be a list of modifier atoms, got: #{inspect(value)}"
  end

  defp normalize_key_match_mode!(value, _owner) when value in [:exact, :all], do: value

  defp normalize_key_match_mode!(value, owner) do
    raise ArgumentError,
          "#{inspect(owner)} expects :match to be :exact or :all, got: #{inspect(value)}"
  end

  defp normalize_event_payload!({pid, _message} = payload, _owner) when is_pid(pid), do: payload

  defp normalize_event_payload!(value, owner) do
    raise ArgumentError,
          "#{inspect(owner)} expects a {pid, message} tuple, got: #{inspect(value)}"
  end

  defp ensure_only_keys!(owner, map, allowed_keys) do
    extras = map |> Map.keys() |> Enum.reject(&(&1 in allowed_keys))

    if extras != [] do
      raise ArgumentError,
            "#{inspect(owner)} does not support key binding fields #{inspect(extras)}"
    end
  end

  defp binding_event_type(:on_key_down), do: :key_down
  defp binding_event_type(:on_key_up), do: :key_up
  defp binding_event_type(:on_key_press), do: :key_press

  defp missing_key_binding_field!(attr) do
    raise ArgumentError,
          "#{inspect(attr)} expects each key binding map to include :key and :payload"
  end

  defp missing_key_matcher_key!(event_type) do
    raise ArgumentError,
          "#{inspect(event_type)} keyword matcher expects a :key entry"
  end

  defp missing_virtual_key_tap! do
    raise ArgumentError, "virtual_key expects a :tap entry"
  end
end
