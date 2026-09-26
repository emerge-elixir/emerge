defmodule EmergeSkia.BundledFontsTest do
  use ExUnit.Case
  use Emerge.UI

  alias EmergeSkia.TreeRenderer

  test "bundled monospace aliases render all four inherited styles without font registration" do
    frames =
      for style <- [[], [Font.bold()], [Font.italic()], [Font.bold(), Font.italic()]] do
        expected = render("monospace", style)

        assert expected == render("JetBrains Mono NL", style)
        assert expected == render("JetBrains Mono", style)
        refute expected == render("default", style)
        assert byte_size(expected) == 260 * 80 * 4
        assert Enum.any?(:binary.bin_to_list(expected), &(&1 != 0))
        expected
      end

    assert length(Enum.uniq(frames)) == 4
  end

  defp render(family, style) do
    el(
      [width(px(260)), height(px(80)), Font.family(family), Font.size(22), Font.color(:white)] ++
        style,
      paragraph([], [el([], text("iiii WWWW 01"))])
    )
    |> TreeRenderer.render_to_pixels([otp_app: :emerge, width: 260, height: 80], 30_000)
  end
end
