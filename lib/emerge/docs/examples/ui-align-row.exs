row(
  [
    width(px(360)),
    height(px(88)),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        align_left(),
        center_y(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Left")
    ),
    el(
      [
        center_x(),
        center_y(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Center")
    ),
    el(
      [
        align_right(),
        center_y(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Right")
    )
  ]
)
