row(
  [
    width(px(340)),
    spacing(16),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(px(140)),
        height(px(72)),
        Transform.move_x(26),
        Transform.move_y(8),
        Background.color(color(:slate, 100)),
        Border.rounded(12),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 800))
      ],
      column([center_x(), center_y(), spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Moved")),
        el([Font.size(11), Font.color(color(:slate, 500))], text("+26px, +8px"))
      ])
    ),
    el(
      [
        width(px(140)),
        height(px(72)),
        Background.color(color(:slate, 50)),
        Border.rounded(12),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 700))
      ],
      column([center_x(), center_y(), spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Layout slot")),
        el([Font.size(11), Font.color(color(:slate, 500))], text("Still placed normally"))
      ])
    )
  ]
)
