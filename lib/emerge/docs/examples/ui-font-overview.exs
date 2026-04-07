column([spacing(16), padding(12), Background.color(color(:slate, 900)), Border.rounded(12)], [
  el(
    [
      padding(12),
      Background.color(color(:slate, 800)),
      Border.rounded(12),
      Font.family("Inter"),
      Font.regular(),
      Font.size(16),
      Font.color(color(:slate, 50))
    ],
    column([spacing(8)], [
      el([Font.semi_bold(), Font.size(20)], text("Release notes")),
      el([Font.color(color(:slate, 300))], text("Design system updated"))
    ])
  ),
  el(
    [
      width(px(280)),
      padding(12),
      Background.color(color(:slate, 50)),
      Border.rounded(10),
      Border.width(1),
      Border.color(color(:slate, 300)),
      Font.color(color(:slate, 800)),
      Font.align_right(),
      Font.extra_light(),
      Font.letter_spacing(1.2)
    ],
    text("ARCHIVE")
  )
])
