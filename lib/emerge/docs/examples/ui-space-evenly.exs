row(
  [
    width(px(360)),
    height(fill()),
    padding(12),
    space_evenly(),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    Input.button(
      [
        padding_xy(12, 8),
        Background.color(color(:slate, 50)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Back")
    ),
    Input.button(
      [
        padding_xy(12, 8),
        Background.color(color(:slate, 100)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Review")
    ),
    Input.button(
      [
        padding_xy(12, 8),
        Background.color(color(:slate, 50)),
        Border.rounded(8),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Ship")
    )
  ]
)
