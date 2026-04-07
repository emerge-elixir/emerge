el(
  [
    width(px(360)),
    height(px(180)),
    padding(20),
    Background.color(color(:slate, 900)),
    Border.rounded(14)
  ],
  el(
    [
      width(px(128)),
      height(px(76)),
      center_x(),
      center_y(),
      Background.color(color_rgba(248, 250, 252, 0.72)),
      Border.rounded(12),
      Border.width(1),
      Border.color(color(:slate, 300)),
      Border.dashed(),
      Font.color(color(:slate, 400)),
      Nearby.in_front(
        el(
          [
            width(fill()),
            height(fill()),
            Transform.move_x(40),
            Transform.move_y(-8),
            Transform.rotate(-10),
            Background.color(color(:slate, 100)),
            Border.rounded(12),
            Border.width(1),
            Border.color(color(:slate, 300)),
            Font.color(color(:slate, 800))
          ],
          column([center_x(), center_y(), spacing(4)], [
            el([Font.size(14), Font.semi_bold()], text("Painted card")),
            el(
              [Font.size(11), Font.color(color(:slate, 500))],
              text("Hit testing follows this")
            )
          ])
        )
      )
    ],
    el([center_x(), center_y(), Font.size(11)], text("Original slot"))
  )
)
