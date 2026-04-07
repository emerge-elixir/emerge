el(
  [
    width(fill()),
    height(fill()),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12),
    center_y()
  ],
  row(
    [
      width(fill()),
      padding(16),
      spacing(16),
      Background.color(color(:slate, 500)),
      Border.rounded(14)
    ],
    [
      el(
        [
          width(px(120)),
          height(px(58)),
          padding(12),
          Background.color(color(:slate, 50)),
          Border.rounded(12),
          Border.shadow(offset: {0, 16}, size: 8, blur: 24, color: color_rgba(15, 23, 42, 0.7)),
          center_y(),
          Font.color(color(:slate, 800))
        ],
        text("Shadow")
      ),
      el(
        [
          width(px(120)),
          height(px(58)),
          padding(12),
          Background.color(color(:slate, 800)),
          Border.rounded(12),
          Border.glow(color_rgba(56, 189, 248, 0.55), 3),
          center_y(),
          Font.color(color(:slate, 50))
        ],
        text("Glow")
      ),
      el(
        [
          width(px(120)),
          height(px(58)),
          padding(12),
          Background.color(color(:slate, 100)),
          Border.rounded(12),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Border.inner_shadow(
            offset: {0, 0},
            size: 8,
            blur: 18,
            color: color_rgba(56, 189, 248, 0.9)
          ),
          center_y(),
          Font.color(color(:slate, 800))
        ],
        text("Inner")
      )
    ]
  )
)
