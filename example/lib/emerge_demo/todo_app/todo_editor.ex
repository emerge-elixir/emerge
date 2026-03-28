defmodule EmergeDemo.TodoApp.TodoEditor do
  @moduledoc false

  use Solve.Controller,
    events: [
      :begin_edit,
      :cancel_edit,
      :set_title,
      :save_edit
    ]

  @impl true
  def init(params, _dependencies), do: %{editing?: false, title: params.title}

  def begin_edit(_payload, state), do: %{state | editing?: true}

  def set_title(title, %{editing?: true} = state) when is_binary(title),
    do: %{state | title: title}

  def set_edit_title(_title, state, _dependencies, _callbacks, _params), do: state

  def cancel_edit(_payload, _state, _dependencies, _callbacks, params),
    do: %{editing?: false, title: params.title}

  def save_edit(_payload, state, _dependencies, callbacks, params) do
    callbacks.save_edit.(params.id, state.title)
    %{editing?: false, title: params.title}
  end

  def expose(state, _dependencies, params), do: Map.put(state, :id, params.id)
end
