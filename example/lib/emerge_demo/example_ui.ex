defmodule EmergeDemo.Counter do
  @moduledoc false

  alias Emerge.UI.Input
  import Emerge.UI


  def init() do
    {:ok, %{counter: subscribe(CounterApp, :counter)}}
  end

  def render(%{counter: counter}) do
    row([], [
      Input.button([on_press(counter.incerment)], text("+")),
      el(text("#{counter.count}")),
      Input.button([on_press(counter.decrement)], text("-")),
    ])
  end
end
