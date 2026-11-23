defmodule EmergeSkia.ParagraphFlowTest do
  use ExUnit.Case, async: true
  use Emerge.UI
  import Emerge.TestSupport.BoxAssertions, only: [button: 3, click: 4, fresh: 3]
  alias Emerge.TestSupport.Headless

  @size {160, 144}
  defp prose(width, words, callback, attrs \\ []) do
    paragraph(
      [
        width(px(width)),
        padding(4),
        Background.color(:white),
        Font.size(16),
        Font.family(:monospace),
        Font.color(color_rgba(0, 0, 0, 0)),
        Event.on_click(:paper) | attrs
      ],
      [
        el(
          [key(:phrase), padding(2), Background.color(:blue), Event.on_click(callback)],
          text(words)
        )
      ]
    )
  end

  defp pixel(frame, x, y), do: binary_part(frame, 4 * (y * elem(@size, 0) + x), 4)

  test "a continuation owns its painted segment, not the empty space above it; one local origin spans the lines" do
    start = fn -> prose(64, "AAA BBB C", :phrase) end
    {pid, r} = Headless.mount(@size, start, [:pointer_button])
    initial = Headless.pixels()
    assert pixel(initial, 48, 8) == <<255, 255, 255, 255>>
    assert pixel(initial, 48, 36) == <<0, 0, 255, 255>>
    click(r, {48.0, 8.0}, :paper, {48.0, 8.0})
    click(r, {48.0, 36.0}, :phrase, {44.0, 32.0})
    button(r, :press, {48.0, 36.0})
    wide = fn -> prose(124, "AAA BBB C", :renamed) end
    send(pid, {:scene, wide})
    changed = Headless.pixels()
    refute changed == initial
    assert pixel(changed, 48, 8) == <<0, 0, 255, 255>>
    button(r, :release, {48.0, 8.0})
    assert_receive {:application, {:renamed, %{x: 44.0, y: 4.0}}}
    fresh(@size, wide, changed)
    send(pid, {:scene, fn -> prose(124, "AAA BBB C", :new_route) end})
    assert Headless.pixels() == changed
    click(r, {48.0, 8.0}, :new_route, {44.0, 4.0})
    send(pid, {:scene, start})
    assert Headless.pixels() == initial
    click(r, {48.0, 36.0}, :phrase, {44.0, 32.0})
    fresh(@size, start, initial)
  end

  test "a kept span moves between paragraphs; floats, scale and inherited font follow the final site" do
    panel = fn id, here, side, scaled, label ->
      f =
        el(
          [
            key({:float, id}),
            side,
            width(px(20)),
            height(px(31)),
            Background.color(:red),
            Event.on_click(id)
          ],
          none()
        )

      words =
        el([key(:kept), padding(2), Background.color(:blue), Event.on_click(:words)], text(label))

      paragraph(
        [
          key(id),
          width(px(70)),
          scale(scaled),
          Font.size(12),
          Font.color(:white),
          Background.color(:black)
        ],
        [f | if(here, do: [words], else: [])]
      )
    end

    scene = fn at, side, scale, label ->
      column([width(fill()), height(fill())], [
        panel.(:a, at == :a, align_left(), 1, label),
        panel.(:b, at == :b, side, scale, label)
      ])
    end

    start = fn -> scene.(:a, align_right(), 1, "AAA BBB CCC") end
    {p, r} = Headless.mount(@size, start, [:pointer_button])
    first = Headless.pixels()
    click(r, {5.0, 5.0}, :a, {5.0, 5.0})
    button(r, :press, {24.0, 5.0})
    moved = fn -> scene.(:b, align_right(), 1.5, "AAA BBB CCC") end
    send(p, {:scene, moved})
    bytes = Headless.pixels()
    refute bytes == first
    button(r, :release, {6.0, 37.0})
    assert_receive {:application, {:words, %{x: 4.0, y: 4.0}}}
    fresh(@size, moved, bytes)

    for {side, scale, label} <- [
          {align_left(), 1.5, "AAA BBB CCC"},
          {align_left(), 1.5, "AA\n\u200b\u2003B longword"},
          {align_right(), 1, "A B"},
          {align_right(), 1.5, "AAA BBB CCC"}
        ] do
      build = fn -> scene.(:b, side, scale, label) end
      send(p, {:scene, build})
      fresh(@size, build, Headless.pixels())
    end

    send(p, {:scene, start})
    assert Headless.pixels() == first
    # The last continuation starts to the left of the float, at the paragraph's x=0.
    click(r, {24.0, 5.0}, :words, {24.0, 5.0})
  end

  test "a float's available width is not a final share: its minimum can overflow the paragraph" do
    build = fn length, side ->
      paragraph([width(px(82)), padding(3), Font.size(14), Background.color(:white)], [
        el(
          [
            side,
            width(length),
            Border.width(3),
            Border.color(:blue),
            Background.color(gradient([:red, :blue])),
            Event.on_click(:float)
          ],
          text("AAA BBB CCC")
        ),
        text("red blue black green white")
      ])
    end

    start = fn -> build.(max(px(104), fill()), align_left()) end
    {pid, r} = Headless.mount(@size, start, [:pointer_button])
    initial = Headless.pixels()

    fresh(@size, fn -> build.(px(104), align_left()) end, initial)
    send(pid, {:scene, fn -> build.(max(px(104), fill()), align_right()) end})
    fresh(@size, fn -> build.(px(104), align_right()) end, Headless.pixels())
    click(r, {60.0, 8.0}, :float, {57.0, 5.0})

    send(pid, {:scene, start})
    assert Headless.pixels() == initial
  end

  test "rounded host and inline corners clip input; translucent gradients and synthetic faces survive equal-size edits and undo" do
    scene = fn kind, ink, word ->
      paragraph(
        [
          width(px(116)),
          height(px(72)),
          Border.rounded(12),
          Border.color(:red),
          Border.width(2),
          Background.color(:white),
          Event.on_click(:paper),
          Font.color(ink)
        ],
        [
          el(
            [
              padding(4),
              Border.rounded(8),
              Background.color(:blue),
              Font.underline(),
              Font.strike(),
              Font.italic(),
              kind,
              Event.on_click(:link)
            ],
            text(word)
          )
        ]
      )
    end

    grad = gradient([:red, color_rgba(255, 255, 255, 0.6), :green], 35)
    base = fn -> scene.(Font.bold(), grad, "AAA BBB CCCC") end
    {p, r} = Headless.mount(@size, base, [:pointer_button])
    first = Headless.pixels()
    click(r, {16.0, 8.0}, :link, {14.0, 6.0})

    for {weight, ink, word} <- [
          {Font.semi_bold(), grad, "AAA BBB CCCC"},
          {Font.bold(), :white, "AZA BXB CXXC"},
          {Font.light(), gradient([:blue, :white]), "ZZZ WWW AAAA"},
          {Font.bold(), grad, "AAA BBB CCCC"}
        ] do
      build = fn -> scene.(weight, ink, word) end
      send(p, {:scene, build})
      bytes = Headless.pixels()
      fresh(@size, build, bytes)

      if word == "AAA BBB CCCC" and weight == Font.bold(),
        do: assert(bytes == first),
        else: refute(bytes == first)
    end

    # Positively witness the button at the surface, without awarding the
    # transparent (rounded-off) corner to the paper or its text wrapper.
    for phase <- [:press, :release], do: button(r, phase, {2.0, 2.0})
    click(r, {16.0, 8.0}, :link, {14.0, 6.0})
    refute_received {:application, {_, %{x: _, y: _}}}
  end
end
