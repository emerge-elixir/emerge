column(
  [
    width(px(380)),
    height(fill()),
    padding(16),
    spacing(14),
    Background.color(color(:slate, 900)),
    Border.rounded(14)
  ],
  [
    el([Font.size(20), Font.color(color(:slate, 50))], text("Project Alpha")),
    el(
      [Font.color(color(:slate, 300))],
      text("A small UI tree built from column/2, el/2, row/2, and text/1.")
    ),
    row([spacing(12)], [
      el(
        [
          padding_xy(12, 8),
          Background.color(color(:slate, 50)),
          Border.rounded(8),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("4 tasks")
      ),
      el(
        [
          padding_xy(12, 8),
          Background.color(color(:sky, 500)),
          Border.rounded(8),
          Font.color(color(:slate, 50))
        ],
        text("Open")
      )
    ])
  ]
)
