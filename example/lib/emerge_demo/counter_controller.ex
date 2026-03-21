defmodule EmergeDemo.CounterController do
  @moduledoc false

  use Solve.Controller, events: [:increment, :decrement]

  @impl true
  def init(_params, _dependencies) do
    %{count: 0}
  end

  def increment(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: state.count + 1}
  end

  def decrement(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: state.count - 1}
  end
end
