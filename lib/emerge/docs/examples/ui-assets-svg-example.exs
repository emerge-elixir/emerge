row(
  [
    width(fill()),
    height(fill()),
    padding(16),
    spacing(12),
    Background.color(color(:slate, 900)),
    Border.rounded(14)
  ],
  [
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 800)),
        Border.rounded(12)
      ],
      column([center_x(), spacing(8)], [
        svg([width(px(48)), height(px(48))], "demo_images/template_cloud.svg"),
        el([Font.color(color(:slate, 50)), Font.size(13)], text("Original SVG"))
      ])
    ),
    el(
      [
        width(fill()),
        padding(12),
        Background.color(color(:slate, 800)),
        Border.rounded(12)
      ],
      column([center_x(), spacing(8)], [
        svg(
          [width(px(48)), height(px(48)), Svg.color(color(:sky, 500))],
          "demo_images/template_cloud.svg"
        ),
        el([Font.color(color(:slate, 50)), Font.size(13)], text("Svg.color/1"))
      ])
    )
  ]
)
