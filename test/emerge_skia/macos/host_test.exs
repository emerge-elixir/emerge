defmodule EmergeSkia.Macos.HostTest do
  use ExUnit.Case, async: true

  alias Emerge.Runtime.Viewport.Renderer, as: ViewportRenderer
  alias EmergeSkia.Macos.Host

  test "handle_call running uses cached session state" do
    state = %{sessions: %{1 => %{running: true}}}

    assert {:reply, true, ^state} = Host.handle_call({:running, 1}, self(), state)
  end

  test "handle_call running returns false for stopped or unknown sessions" do
    state = %{sessions: %{1 => %{running: false}}}

    assert {:reply, false, ^state} = Host.handle_call({:running, 1}, self(), state)
    assert {:reply, false, ^state} = Host.handle_call({:running, 2}, self(), state)
  end

  test "set_input_target sends generic renderer heartbeat" do
    heartbeat = ViewportRenderer.heartbeat_message()

    state = %{
      sessions: %{
        1 => %{
          running: true,
          input_target: nil,
          log_target: nil,
          input_mask: 0xFF,
          input_ready: false,
          pending_resize: nil,
          pending_focus: nil,
          pending_close: false,
          pending_logs: [],
          pending_element_events: []
        }
      }
    }

    assert {:reply, :ok, _state} = Host.handle_call({:set_input_target, 1, self()}, self(), state)
    assert_receive ^heartbeat
  end
end
