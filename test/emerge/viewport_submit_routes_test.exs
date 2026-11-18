defmodule Emerge.Runtime.Viewport.SubmitRoutesTest do
  use ExUnit.Case, async: true
  alias Emerge.Runtime.Viewport.State

  test "a late callback edit cannot enter an earlier successful or failed presentation" do
    a = %State{module: __MODULE__, drawn_routes: %{1 => :a, 2 => :old}}
    sealed = a |> State.routes({:edit, %{1 => :b, 2 => nil}}) |> State.routes(:seal)
    later = State.routes(sealed, {:edit, %{1 => :c}})
    assert later.drawn_routes == a.drawn_routes
    success = State.routes(later, :publish)
    assert success.drawn_routes == %{1 => :b}
    failure = State.routes(later, :retry)
    assert failure.drawn_routes == a.drawn_routes

    for state <- [success, failure] do
      assert state |> State.routes(:seal) |> State.routes(:publish) |> Map.fetch!(:drawn_routes) ==
               %{1 => :c}
    end

    assert sealed |> State.routes(:retry) |> State.routes(:install) |> Map.fetch!(:drawn_routes) ==
             %{1 => :b}
  end
end
