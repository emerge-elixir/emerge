row(
  [
    width(fill()),
    height(fill()),
    padding(12),
    spacing(8),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(fill(1)),
        padding(8),
        Background.color(color(:slate, 50)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("1")
    ),
    el(
      [
        width(fill(2)),
        padding(8),
        Background.color(color(:slate, 100)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("2")
    ),
    el(
      [
        width(fill(3)),
        padding(8),
        Background.color(color(:slate, 200)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("3")
    )
  ]
)
