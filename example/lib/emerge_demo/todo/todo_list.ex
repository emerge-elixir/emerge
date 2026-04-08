defmodule EmergeDemo.Todo.TodoList do
  @moduledoc false

  use Solve.Controller,
    events: [
      :create_todo,
      :update_todo,
      :delete_todo,
      :toggle_todo,
      :toggle_all,
      :clear_completed
    ]

  @impl Solve.Controller
  def init(_params, _dependencies) do
    %{
      next_id: 1,
      ids: [],
      todos: %{}
    }
  end

  def create_todo(title, state, _dependencies, _callbacks, _params) when is_binary(title) do
    case String.trim(title) do
      "" ->
        state

      title ->
        id = state.next_id
        todo = %{id: id, title: title, completed?: false}

        %{
          next_id: id + 1,
          ids: state.ids ++ [id],
          todos: Map.put(state.todos, id, todo)
        }
    end
  end

  def update_todo(%{id: id, title: title}, state, _dependencies, _callbacks, _params)
      when is_binary(title) do
    title = String.trim(title)

    case {Map.has_key?(state.todos, id), title} do
      {false, _} -> state
      {true, ""} -> delete_todo(id, state, %{}, %{}, %{})
      {true, _} -> put_in(state.todos[id].title, title)
    end
  end

  def delete_todo(id, state, _dependencies, _callbacks, _params) do
    %{
      state
      | ids: Enum.reject(state.ids, &(&1 == id)),
        todos: Map.delete(state.todos, id)
    }
  end

  def toggle_todo(id, state, _dependencies, _callbacks, _params) do
    get_and_update_in(state, [:todos, id], fn
      nil -> :pop
      todo -> {todo, %{todo | completed?: !todo.completed?}}
    end)
    |> elem(1)
  end

  def toggle_all(_payload, %{ids: []} = state, _dependencies, _callbacks, _params), do: state

  def toggle_all(_payload, state, _dependencies, _callbacks, _params) do
    mark_completed? = Enum.any?(state.ids, fn id -> not state.todos[id].completed? end)

    todos =
      Map.new(state.ids, fn id ->
        todo = state.todos[id]
        {id, %{todo | completed?: mark_completed?}}
      end)

    %{state | todos: todos}
  end

  def clear_completed(_payload, state, _dependencies, _callbacks, _params) do
    ids = Enum.reject(state.ids, fn id -> state.todos[id].completed? end)
    todos = Map.take(state.todos, ids)
    %{state | ids: ids, todos: todos}
  end

  @impl Solve.Controller
  def expose(state, _dependencies, _params) do
    active_count = Enum.count(state.ids, fn id -> not state.todos[id].completed? end)
    total_count = length(state.ids)
    completed_count = total_count - active_count

    %{
      ids: state.ids,
      todos: state.todos,
      total_count: total_count,
      active_count: active_count,
      completed_count: completed_count,
      all_completed?: total_count > 0 and active_count == 0,
      has_completed?: completed_count > 0,
      empty?: total_count == 0
    }
  end
end
