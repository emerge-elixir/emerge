defmodule EmergeDemoTest do
  use ExUnit.Case, async: false

  defmodule FakeRenderer do
    @behaviour Emerge.Viewport.Renderer

    @impl true
    def start(_skia_opts, _renderer_opts) do
      Agent.start_link(fn -> %{ops: [], running?: true} end)
    end

    @impl true
    def stop(renderer) do
      Agent.stop(renderer)
      :ok
    catch
      :exit, _reason -> :ok
    end

    @impl true
    def running?(renderer), do: Agent.get(renderer, & &1.running?)

    @impl true
    def set_input_target(renderer, pid) do
      Agent.update(renderer, &log_op(&1, {:set_input_target, pid}))
      :ok
    end

    @impl true
    def set_input_mask(renderer, mask) do
      Agent.update(renderer, &log_op(&1, {:set_input_mask, mask}))
      :ok
    end

    @impl true
    def upload_tree(renderer, tree) do
      diff_state = Emerge.diff_state_new(tree)
      Agent.update(renderer, &log_op(&1, {:upload_tree, diff_state.tree}))
      {diff_state, diff_state.tree}
    end

    @impl true
    def patch_tree(renderer, diff_state, tree) do
      {_patch_bin, next_state, assigned_tree} = Emerge.diff_state_update(diff_state, tree)
      Agent.update(renderer, &log_op(&1, {:patch_tree, assigned_tree}))
      {next_state, assigned_tree}
    end

    def ops(renderer), do: Agent.get(renderer, &Enum.reverse(&1.ops))

    defp log_op(state, op), do: %{state | ops: [op | state.ops]}
  end

  setup do
    pid =
      case Process.whereis(EmergeDemo.State) do
        nil ->
          {:ok, pid} = EmergeDemo.State.start_link(name: EmergeDemo.State)
          pid

        pid ->
          pid
      end

    on_exit(fn ->
      if Process.alive?(pid) do
        Process.exit(pid, :normal)
      end
    end)

    :ok
  end

  test "counter render includes solve dispatch refs" do
    tree = EmergeDemo.render(%{})

    assert tree.type == :row
    assert length(tree.children) == 3

    [increment_button, count_label, decrement_button] = tree.children

    assert increment_button.attrs.on_press ==
             {self(),
              %Solve.Message{
                type: :dispatch,
                payload: %Solve.Dispatch{
                  app: EmergeDemo.State,
                  controller_name: :counter,
                  event: :increment,
                  payload: %{}
                }
              }}

    assert decrement_button.attrs.on_press ==
             {self(),
              %Solve.Message{
                type: :dispatch,
                payload: %Solve.Dispatch{
                  app: EmergeDemo.State,
                  controller_name: :counter,
                  event: :decrement,
                  payload: %{}
                }
              }}

    [text_node] = count_label.children
    assert text_node.attrs.content == "Count: 0"
  end

  test "counter dispatch message updates solve state and rerenders without crashing viewport" do
    {:ok, pid} =
      EmergeDemo.start_link(
        viewport: [renderer_module: FakeRenderer, renderer_check_interval_ms: nil]
      )

    on_exit(fn ->
      if Process.alive?(pid) do
        Process.exit(pid, :normal)
      end
    end)

    [increment_button, _count_label, _decrement_button] =
      :sys.get_state(pid).diff_state.tree.children

    {^pid, increment_message} = increment_button.attrs.on_press

    send(pid, increment_message)

    assert_eventually(fn ->
      case Solve.subscribe(EmergeDemo.State, :counter, self()) do
        %{count: 1} -> true
        _ -> false
      end
    end)

    assert_eventually(fn ->
      [_, count_label, _] = :sys.get_state(pid).diff_state.tree.children
      match?([%Emerge.Element{attrs: %{content: "Count: 1"}}], count_label.children)
    end)

    assert Process.alive?(pid)
  end

  test "handle_solve_updated schedules viewport rerender" do
    state = %Emerge.Viewport.State{module: EmergeDemo, mount_opts: [], user_state: %{}}

    assert {:ok, next_state} =
             EmergeDemo.handle_solve_updated(%{EmergeDemo.State => [:counter]}, state)

    assert next_state.dirty?
    assert next_state.flush_scheduled?
    assert_receive {:"$gen_cast", {:emerge_viewport, :flush}}
  end

  defp assert_eventually(assertion, attempts \\ 50)

  defp assert_eventually(_assertion, 0) do
    flunk("condition was not met in time")
  end

  defp assert_eventually(assertion, attempts) do
    if assertion.() do
      :ok
    else
      Process.sleep(10)
      assert_eventually(assertion, attempts - 1)
    end
  end
end
