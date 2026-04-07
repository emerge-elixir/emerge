row(
  [
    width(fill()),
    height(fill()),
    padding(12),
    spacing(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(shrink()),
        padding(10),
        Background.color(color(:slate, 50)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Shrink")
    ),
    el(
      [
        width(fill()),
        padding(10),
        Background.color(color(:slate, 100)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Fill")
    )
  ]
)
