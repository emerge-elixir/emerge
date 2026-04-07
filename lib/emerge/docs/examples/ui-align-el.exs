el(
  [
    width(px(320)),
    height(px(160)),
    padding(16),
    Background.color(color(:slate, 900)),
    Border.rounded(12),
    center_x(),
    center_y()
  ],
  el(
    [
      padding_xy(12, 8),
      Background.color(color(:slate, 50)),
      Border.rounded(999),
      Border.width(1),
      Border.color(color(:slate, 300)),
      Font.color(color(:slate, 800))
    ],
    text("Centered child")
  )
)
