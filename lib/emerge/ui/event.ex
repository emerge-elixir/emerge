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
end
