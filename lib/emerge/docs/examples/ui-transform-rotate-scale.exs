row(
  [
    width(px(360)),
    spacing(24),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(px(150)),
        height(px(84)),
        Transform.rotate(-10),
        Background.color(color(:slate, 100)),
        Border.rounded(14),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      column([center_x(), center_y(), spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Rotate")),
        el([Font.size(11), Font.color(color(:slate, 500))], text("Around the center"))
      ])
    ),
    el(
      [
        width(px(150)),
        height(px(84)),
        Transform.scale(1.14),
        Background.color(color(:slate, 50)),
        Border.rounded(14),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      column([center_x(), center_y(), spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Scale")),
        el(
          [Font.size(11), Font.color(color(:slate, 500))],
          text("Same slot, bigger paint")
        )
      ])
    )
  ]
)
