defmodule EmergeDemo.CounterController do
  @moduledoc false

  use Solve.Controller, events: [:increment, :decrement]

  @min_limit 1
  @max_limit 99

  @impl true
  def init(_params, _dependencies) do
    %{count: 1}
  end

  def increment(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: min(@max_limit, state.count + 1)}
  end

  def decrement(_payload, state, _dependencies, _callbacks, _init_params) do
    %{state | count: max(@min_limit, state.count - 1)}
  end

  @impl true
  def expose(%{count: count}, _, _init_params) do
    %{count: count, can_increment?: count < @max_limit, can_decrement?: count > @min_limit}
  end
end
