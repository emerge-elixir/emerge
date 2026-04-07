column(
  [
    width(px(220)),
    height(px(240)),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        align_top(),
        center_x(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Top")
    ),
    el(
      [
        center_y(),
        center_x(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Middle")
    ),
    el(
      [
        align_bottom(),
        center_x(),
        padding_xy(10, 6),
        Background.color(color(:slate, 50)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      text("Bottom")
    )
  ]
)
