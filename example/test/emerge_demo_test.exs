defmodule EmergeDemoTest do
  use ExUnit.Case, async: true

  test "handle_solve_updated schedules viewport rerender" do
    state = %Emerge.Viewport.State{module: EmergeDemo, mount_opts: [], user_state: %{}}

    assert {:ok, next_state} =
             EmergeDemo.handle_solve_updated(%{EmergeDemo.State => [:counter]}, state)

    assert next_state.dirty?
    assert next_state.flush_scheduled?
    assert_receive {:"$gen_cast", {:emerge_viewport, :flush}}
  end

  test "dev children include the hot reloader" do
    assert [{Emerge.CodeReloader, opts}] =
             EmergeDemo.Application.children(:dev)
             |> Enum.filter(fn
               {Emerge.CodeReloader, _opts} -> true
               _other -> false
             end)

    assert opts[:reloadable_apps] == [:emerge_demo]
    assert Enum.all?(opts[:dirs], &is_binary/1)
  end
end
