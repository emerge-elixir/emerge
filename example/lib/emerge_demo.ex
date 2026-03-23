defmodule EmergeDemo do
  @moduledoc """
  Example application that composes `Solve` state management with
  an `Emerge` viewport process.
  """

  use Emerge
  use Solve.Lookup

  alias Emerge.UI.Animation

  @impl Viewport
  def mount(opts) do
    {:ok, %{}, Keyword.merge([title: "Emerge Demo"], opts)}
  end

  @impl Viewport
  def render(_state) do
    counter = solve(EmergeDemo.State, :counter)
    events = events(counter)

    el(
      [
        Animation.animate(
          [
            [Background.color(color(:gray, 950))],
            [Background.color(color(:gray, 800))],
            [Background.color(color(:gray, 950))]
          ],
          5000,
          :linear,
          :loop
        ),
        width(fill()),
        height(fill())
      ],
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
        Event.on_press(on_press),
        center_y()
      ],
      text(text)
    )
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.rerender(state)}
  end
end
