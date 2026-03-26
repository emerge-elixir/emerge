defmodule Emerge.UI.Event do
  @moduledoc "Event handler helpers for interactive elements."

  @type payload :: {pid(), term()}
  @type click_attr :: {:on_click, payload()}
  @type press_attr :: {:on_press, payload()}
  @type mouse_down_attr :: {:on_mouse_down, payload()}
  @type mouse_up_attr :: {:on_mouse_up, payload()}
  @type mouse_enter_attr :: {:on_mouse_enter, payload()}
  @type mouse_leave_attr :: {:on_mouse_leave, payload()}
  @type mouse_move_attr :: {:on_mouse_move, payload()}
  @type change_attr :: {:on_change, payload()}
  @type focus_attr :: {:on_focus, payload()}
  @type blur_attr :: {:on_blur, payload()}

  @type key_modifier :: :shift | :ctrl | :alt | :meta
  @type key_match_mode :: :exact | :all

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

  @type key_matcher ::
          key_name()
          | [key: key_name(), mods: [key_modifier()], match: key_match_mode()]

  @type key_binding :: %{
          required(:key) => key_name(),
          required(:mods) => [key_modifier()],
          required(:match) => key_match_mode(),
          required(:payload) => payload(),
          required(:route) => binary()
        }

  @type key_binding_descriptor :: %{
          required(:key) => key_name(),
          required(:mods) => [key_modifier()],
          required(:match) => key_match_mode(),
          required(:route) => binary()
        }

  @type key_down_attr :: {:on_key_down, key_binding()}
  @type key_up_attr :: {:on_key_up, key_binding()}
  @type key_press_attr :: {:on_key_press, key_binding()}

  @type t ::
          click_attr()
          | press_attr()
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

  @key_modifiers [:shift, :ctrl, :alt, :meta]

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

  @doc "Register a click handler payload for this element"
  @spec on_click(payload() | term()) :: click_attr()
  def on_click({pid, _msg} = payload) when is_pid(pid), do: {:on_click, payload}
  def on_click(message), do: on_click({self(), message})

  @doc "Register a press handler payload for this element"
  @spec on_press(payload() | term()) :: press_attr()
  def on_press({pid, _msg} = payload) when is_pid(pid), do: {:on_press, payload}
  def on_press(message), do: on_press({self(), message})

  @doc "Register a mouse down handler payload for this element"
  @spec on_mouse_down(payload() | term()) :: mouse_down_attr()
  def on_mouse_down({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_down, payload}
  def on_mouse_down(message), do: on_mouse_down({self(), message})

  @doc "Register a mouse up handler payload for this element"
  @spec on_mouse_up(payload() | term()) :: mouse_up_attr()
  def on_mouse_up({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_up, payload}
  def on_mouse_up(message), do: on_mouse_up({self(), message})

  @doc "Register a mouse enter handler payload for this element"
  @spec on_mouse_enter(payload() | term()) :: mouse_enter_attr()
  def on_mouse_enter({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_enter, payload}
  def on_mouse_enter(message), do: on_mouse_enter({self(), message})

  @doc "Register a mouse leave handler payload for this element"
  @spec on_mouse_leave(payload() | term()) :: mouse_leave_attr()
  def on_mouse_leave({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_leave, payload}
  def on_mouse_leave(message), do: on_mouse_leave({self(), message})

  @doc "Register a mouse move handler payload for this element"
  @spec on_mouse_move(payload() | term()) :: mouse_move_attr()
  def on_mouse_move({pid, _msg} = payload) when is_pid(pid), do: {:on_mouse_move, payload}
  def on_mouse_move(message), do: on_mouse_move({self(), message})

  @doc "Register a change handler payload for this input element"
  @spec on_change(payload() | term()) :: change_attr()
  def on_change({pid, _msg} = payload) when is_pid(pid), do: {:on_change, payload}
  def on_change(message), do: on_change({self(), message})

  @doc "Register a focus handler payload for this input element"
  @spec on_focus(payload() | term()) :: focus_attr()
  def on_focus({pid, _msg} = payload) when is_pid(pid), do: {:on_focus, payload}
  def on_focus(message), do: on_focus({self(), message})

  @doc "Register a blur handler payload for this input element"
  @spec on_blur(payload() | term()) :: blur_attr()
  def on_blur({pid, _msg} = payload) when is_pid(pid), do: {:on_blur, payload}
  def on_blur(message), do: on_blur({self(), message})

  @doc "Register a focused key-down handler for this element"
  @spec on_key_down(key_matcher(), payload() | term()) :: key_down_attr()
  def on_key_down(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_down, build_key_binding!(:key_down, matcher, payload)}
  end

  def on_key_down(matcher, message), do: on_key_down(matcher, {self(), message})

  @doc "Register a focused key-up handler for this element"
  @spec on_key_up(key_matcher(), payload() | term()) :: key_up_attr()
  def on_key_up(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_up, build_key_binding!(:key_up, matcher, payload)}
  end

  def on_key_up(matcher, message), do: on_key_up(matcher, {self(), message})

  @doc "Register a focused completed key-press handler for this element"
  @spec on_key_press(key_matcher(), payload() | term()) :: key_press_attr()
  def on_key_press(matcher, {pid, _msg} = payload) when is_pid(pid) do
    {:on_key_press, build_key_binding!(:key_press, matcher, payload)}
  end

  def on_key_press(matcher, message), do: on_key_press(matcher, {self(), message})

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
end
