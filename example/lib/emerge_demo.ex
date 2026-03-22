defmodule EmergeDemo do
  @moduledoc """
  Example application that composes `Solve` state management with
  an `Emerge.Viewport` renderer process.
  """

  use Solve.Lookup
  use Emerge.Viewport

  import Emerge.Color
  alias Emerge.UI.{Background, Border, Font}

  @impl Viewport
  def mount(opts) do
    {:ok, %{}, Keyword.merge([title: "Emerge Demo"], opts)}
  end

  @impl Viewport
  def render(_state) do
    counter = solve(EmergeDemo.State, :counter)
    events = events(counter)

    el(
      [width(fill()), height(fill())],
      row(
        [
          center_y(),
          center_x(),
          Border.rounded(6),
          Background.color(color(:slate, 800)),
          padding(10),
          spacing(10)
        ],
        [
          button("+", events[:increment]),
          el(
            [Background.color(color(:slate, 700)), center_y(), Font.color(color(:white))],
            text("Count: #{counter.count}")
          ),
          button("-", events[:decrement])
        ]
      )
    )
  end

  def button(text, on_press) do
    Input.button(
      [
        padding(2),
        Border.rounded(6),
        Background.color(color(:slate)),
        Font.center(),
        width(px(50)),
        on_press(on_press),
        center_y()
      ],
      text(text)
    )
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.schedule_rerender(state)}
  end
end
