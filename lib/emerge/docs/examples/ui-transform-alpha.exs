row(
  [
    width(px(320)),
    spacing(12),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  [
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 50)),
        Border.rounded(12),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Font.color(color(:slate, 900))
      ],
      column([spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Primary")),
        el([Font.size(11), Font.color(color(:slate, 500))], text("Fully visible"))
      ])
    ),
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 50)),
        Border.rounded(12),
        Border.width(1),
        Border.color(color(:slate, 300)),
        Transform.alpha(0.45),
        Font.color(color(:slate, 900))
      ],
      column([spacing(4)], [
        el([Font.size(14), Font.semi_bold()], text("Archived")),
        el([Font.size(11), Font.color(color(:slate, 500))], text("Same layout, lower emphasis"))
      ])
    )
  ]
)
