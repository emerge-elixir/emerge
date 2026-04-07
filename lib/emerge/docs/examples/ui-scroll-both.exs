el(
  [
    width(px(320)),
    height(px(180)),
    padding(12),
    scrollbar_x(),
    scrollbar_y(),
    Background.color(color(:slate, 900)),
    Border.rounded(12)
  ],
  el(
    [
      width(px(640)),
      height(px(360)),
      padding(18),
      Background.color(color(:slate, 50)),
      Border.rounded(16),
      Border.width(1),
      Border.color(color(:slate, 300))
    ],
    column([spacing(14)], [
      el([Font.size(18), Font.color(color(:slate, 900))], text("Oversized canvas")),
      row([spacing(12)], [
        el(
          [
            width(px(180)),
            height(px(110)),
            Background.color(color(:slate, 100)),
            Border.rounded(12)
          ],
          none()
        ),
        el(
          [
            width(px(220)),
            height(px(110)),
            Background.color(color(:slate, 50)),
            Border.rounded(12),
            Border.width(1),
            Border.color(color(:slate, 300))
          ],
          none()
        )
      ]),
      row([spacing(12)], [
        el(
          [
            width(px(280)),
            height(px(140)),
            Background.color(color(:slate, 100)),
            Border.rounded(12)
          ],
          none()
        ),
        el(
          [
            width(px(240)),
            height(px(140)),
            Background.color(color(:slate, 50)),
            Border.rounded(12),
            Border.width(1),
            Border.color(color(:slate, 300))
          ],
          none()
        )
      ])
    ])
  )
)
