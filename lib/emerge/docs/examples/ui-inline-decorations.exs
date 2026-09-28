el(
  [width(fill()), height(fill()), padding(24), Background.color(color(:slate, 900))],
  paragraph([width(fill()), Font.size(24), Font.color(:white), center_x()], [
    text("Decorations follow "),
    el(
      [
        padding(3),
        Border.width_each(1, 2, 3, 1),
        Border.rounded_each(8, 0, 8, 0),
        Border.dashed(),
        Border.color(gradient([color(:cyan, 300), color(:blue, 500)])),
        Border.glow(color_rgba(43, 145, 203, 0.4), 2),
        Border.inner_shadow(size: 1, blur: 2, color: color_rgba(43, 145, 203, 0.4))
      ],
      text("inline wrappers across several wrapped lines")
    ),
    text(" without recoloring the text.")
  ])
)
