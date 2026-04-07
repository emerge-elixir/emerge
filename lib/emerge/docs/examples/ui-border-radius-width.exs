row(
  [
    height(fill()),
    spacing(16),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(px(148)),
        padding(12),
        Background.color(color(:slate, 50)),
        Font.color(color(:slate, 800)),
        Border.rounded(14)
      ],
      text("Rounded only")
    ),
    el(
      [
        width(px(148)),
        padding(12),
        Background.color(color(:slate, 50)),
        Border.rounded(14),
        Border.width(3),
        Border.color(color(:sky, 400)),
        Font.color(color(:slate, 800))
      ],
      text("Width + color")
    )
  ]
)
