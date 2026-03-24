defmodule EmergeDemo.CounterController do
  @moduledoc false

  use Solve.Controller, events: [:increment, :decrement]

  @impl true
  def init(_params, _dependencies) do
    %{count: 1}
  end

  def increment(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: min(100, state.count + 1)}
  end

  def decrement(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: max(1, state.count - 1)}
  end

  @impl true
  def expose(%{count: count}, _, _init_params) do
    %{count: count, can_increment?: count < 100, can_decrement?: count > 1}
  end
end
