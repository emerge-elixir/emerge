defmodule EmergeDemo do
  @moduledoc """
  Example application that composes `Solve` state management with
  an `Emerge.Viewport` renderer process.
  """

  use Solve.Lookup
  use Emerge.Viewport

  @impl Viewport
  def mount(opts) do
    {:ok, %{}, Keyword.merge([title: "Emerge Demo"], opts)}
  end

  @impl Viewport
  def render(_state) do
    counter = solve(EmergeDemo.State, :counter)
    events = events(counter)

    row([spacing(10)], [
      Input.button([on_press(events[:increment])], [text("+")]),
      el([], text("Count: #{counter.count}")),
      Input.button([on_press(events[:decrement])], [text("-")])
    ])
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.schedule_rerender(state)}
  end
end
