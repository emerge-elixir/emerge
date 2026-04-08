defmodule EmergeDemo.Todo.Filter do
  @moduledoc false

  use Solve.Controller, events: [:set]

  @filters [:all, :active, :completed]

  @impl true
  def init(_params, _dependencies), do: %{active: :all}

  def set(filter, _state) when filter in @filters, do: %{active: filter}
  def set(_filter, state), do: state

  @impl true
  def expose(state, %{todo_list: todo_list}, _params) do
    %{
      filters: @filters,
      active: state.active,
      visible_ids: visible_ids(state.active, todo_list)
    }
  end

  def label(:all), do: "All"
  def label(:active), do: "Active"
  def label(:completed), do: "Completed"

  defp visible_ids(:all, %{ids: ids}), do: ids

  defp visible_ids(:active, %{ids: ids, todos: todos}),
    do: Enum.reject(ids, fn id -> todos[id].completed? end)

  defp visible_ids(:completed, %{ids: ids, todos: todos}),
    do: Enum.filter(ids, fn id -> todos[id].completed? end)
end
