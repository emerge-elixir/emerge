column(
  [
    padding(10),
    spacing(16),
    Background.color(color(:slate, 900)),
    Border.rounded(16)
  ],
  [
    el(
      [
        width(px(260)),
        padding(16),
        Background.color(color(:slate, 800)),
        Border.rounded(12),
        Font.color(color(:slate, 50))
      ],
      text("Solid panel")
    ),
    el(
      [
        width(px(260)),
        height(px(120)),
        Background.gradient(color(:sky, 400), color(:sky, 700), 45),
        Border.rounded(12)
      ],
      none()
    ),
    el(
      [
        width(px(320)),
        height(px(180)),
        Background.image("sample_assets/static.jpg"),
        Border.rounded(16)
      ],
      none()
    )
  ]
)
