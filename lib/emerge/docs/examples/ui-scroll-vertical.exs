el(
  [
    width(px(240)),
    height(px(180)),
    padding(12),
    scrollbar_y(),
    Background.color(color(:slate, 900)),
    Border.rounded(12),
    Font.color(color(:slate, 50))
  ],
  column([spacing(8)], [
    text("Item 1"),
    text("Item 2"),
    text("Item 3"),
    text("Item 4"),
    text("Item 5"),
    text("Item 6"),
    text("Item 7"),
    text("Item 8"),
    text("Item 9"),
    text("Item 10"),
    text("Item 11"),
    text("Item 12")
  ])
)
