defmodule EmergeDemo do
  @moduledoc """
  Desktop example shell built with `Emerge` and `Solve`.
  """

  use Emerge
  use Solve.Lookup

  @impl Viewport
  def mount(opts) do
    {:ok, Keyword.merge([emerge_skia: [otp_app: :emerge_demo, title: "Emerge Example"]], opts)}
  end

  @impl Viewport
  def render() do
    EmergeDemo.AppSelector.View.layout()
  end

  @impl Solve.Lookup
  def handle_solve_updated(_updated, state) do
    {:ok, Viewport.rerender(state)}
  end
end
