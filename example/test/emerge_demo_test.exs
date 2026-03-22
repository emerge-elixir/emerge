defmodule EmergeDemoTest do
  use ExUnit.Case, async: true

  test "handle_solve_updated schedules viewport rerender" do
    state = %{
      __emerge__: %Emerge.Runtime.Viewport.State{module: EmergeDemo}
    }

    assert {:ok, next_state} =
             EmergeDemo.handle_solve_updated(%{EmergeDemo.State => [:counter]}, state)

    assert next_state.__emerge__.dirty?
    assert next_state.__emerge__.flush_scheduled?
    assert_receive {:"$gen_cast", {:emerge_viewport, :flush}}
  end

  test "dev children include the hot reloader" do
    assert [{Emerge.Runtime.CodeReloader, opts}] =
             EmergeDemo.Application.children(:dev)
             |> Enum.filter(fn
               {Emerge.Runtime.CodeReloader, _opts} -> true
               _other -> false
             end)

    assert opts[:reloadable_apps] == [:emerge_demo]
    assert Enum.all?(opts[:dirs], &is_binary/1)
  end
end
