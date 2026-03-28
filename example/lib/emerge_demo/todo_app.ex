defmodule EmergeDemo.TodoApp do
  @moduledoc """
  Solve implementation of TodoMVC example app
  """

  alias EmergeDemo.TodoApp

  use Solve

  def child_spec(opts) do
    %{
      id: {__MODULE__, Keyword.get(opts, :name, __MODULE__)},
      start: {__MODULE__, :start_link, [opts]},
      type: :worker,
      restart: :permanent
    }
  end

  @impl Solve
  def controllers do
    [
      controller!(name: :todo_list, module: TodoApp.TodoList),
      controller!(
        name: :create_todo,
        module: TodoApp.CreateTodo,
        params: fn _ -> true end,
        callbacks: %{
          submit: fn title -> dispatch(:todo_list, :create_todo, title) end
        }
      ),
      controller!(
        name: :filter,
        module: TodoApp.Filter,
        dependencies: [:todo_list],
        params: fn %{dependencies: %{todo_list: todo_list}} -> not is_nil(todo_list) end
      ),
      controller!(
        name: :todo_editor,
        module: TodoApp.TodoEditor,
        variant: :collection,
        dependencies: [:todo_list],
        params: fn %{dependencies: %{todo_list: todo_list}} -> not is_nil(todo_list) end,
        callbacks: %{
          save_edit: fn id, title ->
            dispatch(:todo_list, :update_todo, %{id: id, title: title})
          end
        },
        collect: fn %{dependencies: %{todo_list: todo_list}} ->
          Enum.map(todo_list.ids, fn id ->
            {id, [params: %{id: id, title: todo_list.todos[id].title}]}
          end)
        end
      )
    ]
  end
end
