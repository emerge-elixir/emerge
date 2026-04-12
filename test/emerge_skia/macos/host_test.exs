defmodule EmergeSkia.Macos.HostTest do
  use ExUnit.Case, async: true

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
end
