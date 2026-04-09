column(
  [
    width(fill()),
    height(fill()),
    padding(16),
    spacing(16),
    Background.color(color(:slate, 900)),
    Border.rounded(14)
  ],
  [
    column([spacing(8)], [
      el([Font.color(color(:slate, 50)), Font.size(14)], text("image/2")),
      el(
        [
          padding(10),
          Background.color(color(:slate, 800)),
          Border.rounded(12)
        ],
        image([width(px(120)), height(px(120)), Border.rounded(10)], "sample_assets/static.jpg")
      )
    ]),
    column([spacing(8)], [
      el([Font.color(color(:slate, 50)), Font.size(14)], text("Background.image/2")),
      el(
        [
          width(px(288)),
          height(px(160)),
          padding(12),
          Background.image("sample_assets/fallback.jpg", fit: :cover),
          Border.rounded(12)
        ],
        column([height(fill()), spacing(8)], [
          el(
            [
              padding_xy(10, 6),
              Background.color(color_rgba(15, 23, 42, 0.7)),
              Border.rounded(999),
              Font.color(color(:slate, 50))
            ],
            text("Featured trail")
          ),
          el(
            [
              align_bottom(),
              padding(10),
              Background.color(color_rgba(15, 23, 42, 0.58)),
              Border.rounded(10),
              Font.color(color(:slate, 50))
            ],
            column([spacing(4)], [
              el([Font.size(18)], text("Background image host")),
              el(
                [Font.size(12), Font.color(color(:slate, 200))],
                text("Foreground content sits on top.")
              )
            ])
          )
        ])
      )
    ])
  ]
)
