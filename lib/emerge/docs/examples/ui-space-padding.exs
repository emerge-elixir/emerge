el(
  [
    width(px(360)),
    padding_xy(16, 12),
    Background.color(color(:slate, 900)),
    Border.rounded(12),
    Font.color(color(:slate, 50))
  ],
  row([width(fill()), spacing(12), center_y()], [
    el(
      [
        padding_each(4, 8, 4, 8),
        Background.color(color(:slate, 100)),
        Border.rounded(999),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.size(12),
        Font.color(color(:slate, 800))
      ],
      text("Stable")
    ),
    column([spacing(4)], [
      el([Font.color(color(:slate, 50))], text("Release branch")),
      el(
        [Font.size(12), Font.color(color(:slate, 300))],
        text("Padding creates the card gutter")
      )
    ])
  ])
)
