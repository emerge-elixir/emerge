defmodule Emerge.Runtime.Viewport.SubmitRoutesTest do
  use ExUnit.Case, async: true
  alias Emerge.Runtime.Viewport.State

  test "speculative, ready and installed callbacks can name the same ID without aliasing" do
    state = %State{module: __MODULE__}
    a = State.register(state, 10, %{1 => :a, 2 => :old})
    b = State.register(a, 11, %{1 => :b})
    c = State.register(b, 12, %{1 => :c})
    assert c.route_epochs[10][2] == :old
    assert c.route_epochs[11][1] == :b
    assert c.route_epochs[12][1] == :c
    # Whether B never started, lost at submission, or failed its handoff,
    # retiring B cannot change the routes of the installed or pending image.
    after_b = State.retire(c, 11)
    assert after_b.route_epochs == %{10 => %{1 => :a, 2 => :old}, 12 => %{1 => :c}}
    refute Map.has_key?(State.retire(after_b, 10).route_epochs, 10)
  end
end
