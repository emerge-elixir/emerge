el(
  [
    width(px(320)),
    padding(12),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  el(
    [
      width(fill()),
      padding(12),
      Background.color(color(:slate, 50)),
      Border.rounded(10),
      Border.width(1),
      Border.color(color(:slate, 300)),
      Font.color(color(:slate, 800))
    ],
    column([width(fill()), spacing(12)], [
      el([width(fill()), Font.align_left()], text("Left aligned body copy")),
      el([width(fill()), Font.center(), Font.italic()], text("Welcome back")),
      el([width(fill()), Font.align_right(), Font.letter_spacing(1.2)], text("12:45 PM")),
      row([spacing(12)], [
        el([Font.underline()], text("Open settings")),
        el([Font.strike()], text("Deprecated"))
      ])
    ])
  )
)
