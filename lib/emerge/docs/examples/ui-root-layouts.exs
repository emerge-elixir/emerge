column(
  [
    width(px(420)),
    height(fill()),
    padding(16),
    spacing(14),
    Background.color(color(:slate, 900)),
    Border.rounded(14)
  ],
  [
    el([Font.size(14), Font.color(color(:slate, 50))], text("row/2 keeps children on one line")),
    row([spacing(8)], [
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("One")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Two")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Three")
      )
    ]),
    el(
      [Font.size(14), Font.color(color(:slate, 50))],
      text("wrapped_row/2 wraps horizontal content onto new lines")
    ),
    wrapped_row([width(px(320)), spacing_xy(8, 8)], [
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Docs")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Layout")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Nearby")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Animation")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Input")
      ),
      el(
        [
          padding_xy(10, 6),
          Background.color(color(:slate, 50)),
          Border.rounded(999),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Scroll")
      )
    ]),
    el(
      [Font.size(14), Font.color(color(:slate, 50))],
      text("column/2 stacks children vertically")
    ),
    column([spacing(8)], [
      el(
        [
          padding(10),
          Background.color(color(:slate, 50)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Header")
      ),
      el(
        [
          padding(10),
          Background.color(color(:slate, 100)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Body")
      ),
      el(
        [
          padding(10),
          Background.color(color(:slate, 50)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color(:slate, 300)),
          Font.color(color(:slate, 800))
        ],
        text("Footer")
      )
    ])
  ]
)
