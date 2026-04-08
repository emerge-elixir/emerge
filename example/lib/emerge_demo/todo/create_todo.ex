defmodule EmergeDemo.Todo.CreateTodo do
  @moduledoc false

  use Solve.Controller, events: [:set_title, :submit]

  @impl true
  def init(_params, _dependencies), do: %{title: ""}

  def set_title(title) when is_binary(title) do
    %{title: title}
  end

  def submit(_payload, state, _dependencies, callbacks) do
    case String.trim(state.title) do
      "" ->
        %{title: ""}

      title ->
        callbacks.submit.(title)
        %{title: ""}
    end
  end
end
