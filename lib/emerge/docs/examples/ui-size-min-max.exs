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
        width(min(px(140), shrink())),
        padding(10),
        Background.color(color(:slate, 50)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("At least 140px")
    ),
    el(
      [
        width(max(px(180), fill())),
        padding(10),
        Background.color(color(:slate, 100)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Fill, but cap at 180px")
    )
  ]
)
