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

    column(
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
      [
        row(
          [
            Border.rounded(6),
            width(fill()),
            Background.color(color(:slate, 800)),
            padding(10)
          ],
          [
            row(
              [center_x(), spacing(12)],
              Enum.concat([
                with button <-
                       if(
                         counter.can_increment?,
                         do: &button/2,
                         else: &button_disabled/2
                       ) do
                  [
                    button.("+", events[:increment]),
                    button.("Plus", events[:increment])
                  ]
                end,
                [
                  el(
                    [padding(10), Background.color(color(:slate, 700)), center_y(), Font.color(color(:white))],
                    text("Count: #{counter.count}")
                  )
                ],
                with button <- if(counter.can_decrement?, do: &button/2, else: &button_disabled/2) do
                  [
                    button.("Minus", events[:decrement]),
                    button.("-", events[:decrement])
                  ]
                end
              ])
            )
          ]
        ),
        row(
          [width(fill())],
          Enum.flat_map(1..36, fn _n ->
            [
              el(
                [
                  Background.color(color(:green, 500)),
                  width(fill(100 - counter.count)),
                  height(px(50))
                ],
                none()
              ),
              el(
                [width(fill(counter.count)), height(px(50))],
                none()
              )
            ]
          end)
        )
      ]
    )
  end

  def button(text, on_press) do
    Input.button(
      [
        padding(20),
        Border.rounded(6),
        Background.color(color(:slate)),
        Font.center(),
        Event.on_press(on_press),
        center_y(),
        Border.shadow(offset: {2,2}, size: 2)
      ],
      text(text)
    )
  end

  def button_disabled(text, _on_press) do
    Input.button(
      [
        padding(20),
        Border.rounded(6),
        Background.color(color(:slate)),
        Font.center(),
        center_y(),
        Transform.alpha(0.4),
        Border.shadow(offset: {2,2}, size: 2),
      ],
      text(text)
    )
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.rerender(state)}
  end
end
