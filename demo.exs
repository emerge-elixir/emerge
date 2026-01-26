# Demo script for EmergeSkia
# Run with: mix run demo.exs

IO.puts("Starting EmergeSkia demo...")

{:ok, renderer} = EmergeSkia.start("EmergeSkia Demo", 800, 600)

EmergeSkia.set_input_target(renderer, self())

defmodule Demo do
  import Emerge.UI

  alias Emerge.UI.{Font, Background, Border}

  @dark_bg {:color_rgba, {26, 26, 46, 255}}
  @blue {:color_rgba, {67, 97, 238, 255}}
  @purple {:color_rgba, {114, 9, 183, 255}}
  @pink {:color_rgba, {247, 37, 133, 255}}
  @light_text {:color_rgba, {255, 255, 255, 255}}
  @dim_text {:color_rgba, {170, 170, 170, 255}}
  @event_bg {:color_rgba, {45, 45, 68, 255}}

  def format_event({:cursor_pos, {x, y}}) do
    "Mouse: #{Float.round(x, 1)}, #{Float.round(y, 1)}"
  end

  def format_event({:cursor_button, {button, action, mods, {x, y}}}) do
    action_str = if action == 1, do: "pressed", else: "released"
    mods_str = if mods == [], do: "", else: " [#{Enum.join(mods, ", ")}]"
    "Click: #{button} #{action_str} at #{Float.round(x, 1)}, #{Float.round(y, 1)}#{mods_str}"
  end

  def format_event({:cursor_scroll, {{dx, dy}, {x, y}}}) do
    "Scroll: #{Float.round(dx, 2)}, #{Float.round(dy, 2)} at #{Float.round(x, 1)}, #{Float.round(y, 1)}"
  end

  def format_event({:key, {key, action, mods}}) do
    action_str = if action == 1, do: "pressed", else: "released"
    mods_str = if mods == [], do: "", else: " [#{Enum.join(mods, ", ")}]"
    "Key: #{key} #{action_str}#{mods_str}"
  end

  def format_event({:cursor_entered, entered}) do
    if entered, do: "Cursor: entered window", else: "Cursor: left window"
  end

  def format_event({:focused, focused}) do
    if focused, do: "Window: focused", else: "Window: unfocused"
  end

  def format_event({:resized, {w, h, scale}}) do
    "Resize: #{w}x#{h} (scale: #{Float.round(scale, 2)})"
  end

  def format_event({id_bin, :click}) when is_binary(id_bin) do
    "Click: element #{inspect(:erlang.binary_to_term(id_bin))}"
  end

  def format_event({id_bin, event}) when is_binary(id_bin) and is_atom(event) do
    label =
      event
      |> Atom.to_string()
      |> String.replace("_", " ")
      |> String.capitalize()

    "Event: #{label} on #{inspect(:erlang.binary_to_term(id_bin))}"
  end

  def format_event(event) do
    inspect(event)
  end

  def format_page(page) when is_atom(page) do
    page
    |> Atom.to_string()
    |> String.replace("_", " ")
    |> String.capitalize()
  end

  def build_tree(
        {_width, _height},
        {mx, my},
        event_log,
        current_page,
        hovered_menu,
        last_move_label,
        unstable_items
      ) do
    column(
      [
        width(:fill),
        height(:fill),
        padding(20),
        spacing(16),
        Background.color(@dark_bg)
      ],
      [
        header_section(mx, my),
        row([width(:fill), height(:fill), spacing(16)], [
          menu_panel(current_page, hovered_menu, event_log),
          content_panel(current_page, last_move_label, unstable_items)
        ]),
        footer_bar(mx, my, event_log)
      ]
    )
  end

  defp header_section(mx, my) do
    row([width(:fill), spacing(16)], [
      el(
        [
          width(:fill),
          padding(16),
          Background.gradient(@blue, @purple, 90),
          Border.rounded(12)
        ],
        column([spacing(6)], [
          el([Font.size(24), Font.color(@light_text)], text("EmergeSkia Demo")),
          el(
            [Font.size(13), Font.color(@dim_text)],
            text("Layout + rendering showcase")
          )
        ])
      ),
      el(
        [
          padding(12),
          Background.color(@event_bg),
          Border.rounded(12)
        ],
        column([spacing(4)], [
          el([Font.size(14), Font.color(@light_text)], text("Live Input")),
          el([Font.size(12), Font.color(@dim_text)], text("X: #{Float.round(mx, 1)}")),
          el([Font.size(12), Font.color(@dim_text)], text("Y: #{Float.round(my, 1)}"))
        ])
      )
    ])
  end

  defp menu_panel(current_page, hovered_menu, _event_log) do
    menu_items = [
      {"Overview", :overview},
      {"Layout", :layout},
      {"Scroll", :scroll},
      {"Alignment", :alignment},
      {"Transforms", :transforms},
      {"Events", :events},
      {"Unstable List", :unstable_list},
      {"Nearby", :nearby}
    ]

    column(
      [
        width(px(220)),
        height(fill()),
        padding(12),
        spacing(12),
        Background.color(@event_bg),
        Border.rounded(12)
      ],
      [
        el([Font.size(16), Font.color(@light_text)], text("Menu")),
        column(
          [spacing(8)],
          Enum.map(menu_items, fn {label, page} ->
            menu_item(label, page, current_page, hovered_menu)
          end)
        ),
        el([Font.size(12), Font.color(@dim_text)], text("Navigation")),
        el([Font.size(11), Font.color(@dim_text)], text("Click pages to switch"))
      ]
    )
  end

  defp content_panel(current_page, last_move_label, unstable_items) do
    el(
      [
        width(fill()),
        height(fill()),
        padding(16),
        scrollbar_y(),
        Background.color({:color_rgb, {35, 35, 55}}),
        Border.rounded(12)
      ],
      render_page(current_page, last_move_label, unstable_items)
    )
  end

  defp footer_bar(mx, my, event_log) do
    row([width(fill()), spacing(12)], [
      el(
        [padding(8), Background.color(@pink), Border.rounded(8)],
        row([spacing(10)], [
          el([Font.size(12), Font.color(@light_text)], text("Cursor")),
          el(
            [Font.size(12), Font.color(@light_text)],
            text("#{Float.round(mx, 1)}, #{Float.round(my, 1)}")
          )
        ])
      ),
      el(
        [
          width(fill()),
          height(px(180)),
          padding(8),
          scrollbar_y(),
          Background.color({:color_rgb, {35, 35, 55}}),
          Border.rounded(8)
        ],
        column(
          [spacing(4)],
          event_log
          |> Enum.take(8)
          |> Enum.reverse()
          |> Enum.map(fn line ->
            el([Font.size(11), Font.color(@dim_text)], text(line))
          end)
        )
      )
    ])
  end

  defp render_page(current_page, last_move_label, unstable_items) do
    case current_page do
      :overview -> page_overview()
      :layout -> page_layout()
      :scroll -> page_scroll()
      :alignment -> page_alignment()
      :transforms -> page_transforms()
      :events -> page_events(last_move_label)
      :unstable_list -> page_unstable_list(unstable_items)
      :nearby -> page_nearby()
      _ -> page_overview()
    end
  end

  defp page_overview() do
    column([width(fill()), spacing(16)], [
      el([Font.size(22), Font.color(:white)], text("Overview")),
      el(
        [Font.size(13), Font.color(@dim_text)],
        text("Explore layout, scrolling, alignment, and transform demos from the menu.")
      ),
      row([width(fill()), spacing(12)], [
        feature_card("Rows", "Horizontal layouts", {:color_rgb, {60, 60, 120}}),
        feature_card("Columns", "Vertical layouts", {:color_rgb, {60, 90, 60}}),
        feature_card("Nesting", "Compose layouts", {:color_rgb, {90, 60, 90}})
      ]),
      el(
        [
          width(fill()),
          padding(14),
          Background.color({:color_rgb, {60, 50, 80}}),
          Border.rounded_each(18, 6, 22, 10)
        ],
        column([spacing(6)], [
          el([Font.size(16), Font.color(:white)], text("Per-corner radius")),
          el(
            [Font.size(12), Font.color({:color_rgb, {200, 200, 220}})],
            text("Each corner can be different")
          )
        ])
      ),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(12),
            Background.color({:color_rgb, {50, 70, 90}}),
            Border.rounded(10),
            rotate(-6),
            alpha(0.85)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Rotate + alpha")),
            el([Font.size(11), Font.color({:color_rgb, {200, 220, 230}})], text("-6deg, 85%"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(12),
            Background.color({:color_rgb, {70, 60, 90}}),
            Border.rounded(10),
            scale(1.06),
            move_y(-14)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Scale + move")),
            el([Font.size(11), Font.color({:color_rgb, {220, 210, 235}})], text("1.06x, -4px"))
          ])
        )
      ]),
      row([width(fill()), spacing(8)], [
        chip("Layout"),
        chip("Scroll"),
        chip("Alignment"),
        chip("Transforms"),
        chip("Nearby")
      ])
    ])
  end

  defp page_layout() do
    column([width(fill()), spacing(16)], [
      section_title("Layout + Sizing"),
      el([Font.size(12), Font.color(@dim_text)], text("Fill, shrink, min/max, and spacing")),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(shrink()),
            padding(10),
            Background.color({:color_rgb, {55, 70, 90}}),
            Border.rounded(8),
            clip()
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Shrink")),
            el([Font.size(11), Font.color({:color_rgb, {210, 220, 230}})], text("Content sized"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(10),
            Background.color({:color_rgb, {70, 80, 95}}),
            Border.rounded(8),
            clip()
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Fill")),
            el([Font.size(11), Font.color({:color_rgb, {220, 225, 235}})], text("Expands"))
          ])
        )
      ]),
      row([width(fill()), spacing(8)], [
        el(
          [
            width(fill_portion(1)),
            padding(8),
            Background.color({:color_rgb, {65, 70, 100}}),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 1"))
        ),
        el(
          [
            width(fill_portion(2)),
            padding(8),
            Background.color({:color_rgb, {65, 80, 110}}),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 2"))
        ),
        el(
          [
            width(fill_portion(3)),
            padding(8),
            Background.color({:color_rgb, {65, 90, 120}}),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 3"))
        )
      ]),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(minimum(140, shrink())),
            padding(10),
            Background.color({:color_rgb, {70, 65, 95}}),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Min + shrink")),
            el([Font.size(11), Font.color({:color_rgb, {220, 220, 235}})], text(">= 140px"))
          ])
        ),
        el(
          [
            width(maximum(180, fill())),
            padding(10),
            Background.color({:color_rgb, {85, 65, 95}}),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Max + fill")),
            el([Font.size(11), Font.color({:color_rgb, {225, 215, 235}})], text("<= 180px"))
          ])
        )
      ]),
      section_title("Spacing + Wrapping"),
      row([width(fill()), space_evenly()], [
        chip("Space"),
        chip("Between"),
        chip("Items")
      ]),
      wrapped_row([width(fill()), spacing_xy(16, 18)], [
        chip("Spacing"),
        chip("X/Y"),
        chip("Wrapped"),
        chip("Row"),
        chip("Example")
      ])
    ])
  end

  defp page_scroll() do
    column([width(fill()), spacing(16)], [
      section_title("Scroll Containers"),
      el([Font.size(12), Font.color(@dim_text)], text("Wheel or drag inside panels")),
      el(
        [
          width(fill()),
          height(px(140)),
          padding(10),
          scrollbar_y(),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(6)
        ],
        column([spacing(6)], [
          el([Font.size(12), Font.color(:white)], text("Scrollable item 1")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 2")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 3")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 4")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 5")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 6")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 7")),
          el([Font.size(12), Font.color(:white)], text("Scrollable item 8"))
        ])
      ),
      section_title("Horizontal Scroll"),
      el(
        [
          width(fill()),
          height(px(90)),
          padding(10),
          scrollbar_x(),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(6)
        ],
        row([spacing(12)], [
          chip("Horiz"),
          chip("Scroll"),
          chip("Example"),
          chip("With"),
          chip("Lots"),
          chip("Of"),
          chip("Chips"),
          chip("To"),
          chip("Move"),
          chip("Around")
        ])
      ),
      section_title("Nested Scroll"),
      el(
        [
          width(fill()),
          height(px(120)),
          padding(10),
          scrollbar_y(),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(6)
        ],
        column([spacing(6)], [
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 1")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 2")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 3")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 4")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 5")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 6")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 7")),
          el([Font.size(12), Font.color(:white)], text("Nested scroll item 8"))
        ])
      )
    ])
  end

  defp page_events(last_move_label) do
    move_label = last_move_label || "None"

    column([width(fill()), spacing(16)], [
      section_title("Mouse Events"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Hover, press, and move inside the cards.")
      ),
      row([width(fill()), spacing(12)], [
        event_card("Mouse Down", :mouse_down, {:color_rgb, {70, 70, 110}}),
        event_card("Mouse Up", :mouse_up, {:color_rgb, {70, 90, 90}})
      ]),
      row([width(fill()), spacing(12)], [
        event_card("Mouse Enter", :mouse_enter, {:color_rgb, {85, 65, 100}}),
        event_card("Mouse Leave", :mouse_leave, {:color_rgb, {90, 70, 60}})
      ]),
      event_card("Mouse Move", :mouse_move, {:color_rgb, {60, 80, 110}}),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Last move target: #{move_label}")
      )
    ])
  end

  defp page_unstable_list(unstable_items) do
    column([width(fill()), spacing(16)], [
      section_title("Unstable Ordering"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Scramble to see clicks follow labels without keys.")
      ),
      el(
        [
          padding(10),
          Background.color(@blue),
          Border.rounded(8),
          on_click({self(), :scramble_unstable})
        ],
        el([Font.size(12), Font.color(:white)], text("Scramble Items"))
      ),
      row([width(fill()), spacing(16)], [
        column([width(fill()), spacing(10)], [
          el([Font.size(12), Font.color(@dim_text)], text("Unstable (no keys)")),
          render_unstable_items(unstable_items, false)
        ]),
        column([width(fill()), spacing(10)], [
          el([Font.size(12), Font.color(@dim_text)], text("Stable (keys)")),
          render_unstable_items(unstable_items, true)
        ])
      ])
    ])
  end

  defp render_unstable_items(items, keyed?) do
    column(
      [spacing(12)],
      Enum.map(items, fn item ->
        row_key = if keyed?, do: [key: {:stable, item.label}], else: []

        column(
          [
            padding(12),
            Background.color({:color_rgb, {50, 50, 75}}),
            Border.rounded(8),
            spacing(8),
            on_click({self(), {:unstable_row_click, item.label}})
          ] ++ row_key,
          [
            el(
              [Font.size(12), Font.color(@light_text)] ++
                if(keyed?, do: [key: {:stable, :header, item.label}], else: []),
              text("#{item.label} (#{item.count})")
            ),
            el(
              [
                width(fill()),
                height(px(90)),
                padding(6),
                scrollbar_y(),
                Background.color({:color_rgb, {40, 40, 60}}),
                Border.rounded(8)
              ] ++ if(keyed?, do: [key: {:stable, :scroll, item.label}], else: []),
              column(
                [spacing(6)],
                Enum.map(item.children, fn child ->
                  child_key =
                    if keyed?, do: [key: {:stable, item.label, child.label}], else: []

                  el(
                    [
                      padding(6),
                      Background.color({:color_rgb, {70, 70, 95}}),
                      Border.rounded(10),
                      on_click({self(), {:unstable_child_click, item.label, child.label}})
                    ] ++ child_key,
                    el(
                      [Font.size(10), Font.color(@light_text)],
                      text("#{child.label} (#{child.count})")
                    )
                  )
                end)
              )
            )
          ]
        )
      end)
    )
  end

  defp page_alignment() do
    column([width(fill()), spacing(16)], [
      section_title("Alignment"),
      row([width(fill()), spacing(10)], [
        el(
          [
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            Font.size(12),
            Font.color(:white)
          ],
          text("Left")
        ),
        el(
          [
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            align_left(),
            Font.size(12),
            Font.color(:white)
          ],
          text("Left 2")
        ),
        el(
          [
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            center_x(),
            Font.size(12),
            Font.color(:white)
          ],
          text("Center")
        ),
        el(
          [
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            align_right(),
            Font.size(12),
            Font.color(:white)
          ],
          text("Right")
        )
      ]),
      row([width(fill()), spacing(10)], [
        el(
          [
            width(px(180)),
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            center_x(),
            Font.size(12),
            Font.color(:white)
          ],
          text("Centered text")
        ),
        el(
          [
            width(px(180)),
            padding(10),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4),
            align_right(),
            Font.size(12),
            Font.color(:white)
          ],
          text("Right-aligned")
        )
      ]),
      row([width(fill())], [
        el(
          [
            width(px(200)),
            padding(10),
            center_x(),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4)
          ],
          el(
            [width(fill()), align_right(), Font.size(12), Font.color(:white)],
            text("Centered box, right text")
          )
        )
      ]),
      el(
        [
          width(fill()),
          height(px(80)),
          padding(10),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(6)
        ],
        el(
          [
            width(fill()),
            height(fill()),
            center_x(),
            center_y(),
            Font.size(16),
            Font.color(:cyan)
          ],
          text("Centered content")
        )
      )
    ])
  end

  defp page_transforms() do
    column([width(fill()), spacing(16)], [
      section_title("Transforms"),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(14),
            Background.color({:color_rgb, {50, 70, 90}}),
            Border.rounded(10),
            rotate(-8),
            alpha(0.8)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Rotate")),
            el([Font.size(11), Font.color({:color_rgb, {200, 220, 230}})], text("-8deg"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(14),
            Background.color({:color_rgb, {70, 60, 90}}),
            Border.rounded(10),
            scale(1.08)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Scale")),
            el([Font.size(11), Font.color({:color_rgb, {220, 210, 235}})], text("1.08x"))
          ])
        )
      ]),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(14),
            Background.color({:color_rgb, {60, 80, 70}}),
            Border.rounded(10),
            move_x(16)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Move")),
            el([Font.size(11), Font.color({:color_rgb, {210, 230, 220}})], text("+16px x"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(14),
            Background.color({:color_rgb, {80, 70, 60}}),
            Border.rounded(10),
            alpha(0.6)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Alpha")),
            el([Font.size(11), Font.color({:color_rgb, {230, 220, 210}})], text("60%"))
          ])
        )
      ])
    ])
  end

  defp page_nearby() do
    column([width(fill()), spacing(16)], [
      section_title("Nearby Elements"),
      el(
        [
          width(fill()),
          height(px(160)),
          padding(15),
          Background.color({:color_rgba, {45, 45, 65, 40}}),
          Border.rounded(6)
        ],
        el(
          [
            width(px(140)),
            height(px(60)),
            center_x(),
            center_y(),
            Background.color({:color_rgb, {70, 70, 120}}),
            Border.rounded(6),
            above(
              el(
                [
                  padding(6),
                  Background.color({:color_rgb, {90, 70, 70}}),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Above")
              )
            ),
            below(
              el(
                [
                  padding(6),
                  Background.color({:color_rgb, {70, 90, 70}}),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Below")
              )
            ),
            on_left(
              el(
                [
                  padding(6),
                  Background.color({:color_rgb, {70, 70, 90}}),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Left")
              )
            ),
            on_right(
              el(
                [
                  padding(6),
                  Background.color({:color_rgb, {90, 90, 70}}),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Right")
              )
            ),
            behind_content(
              el(
                [
                  width(px(160)),
                  height(px(70)),
                  Background.color({:color_rgba, {200, 200, 255, 40}}),
                  Border.rounded(8)
                ],
                text("Behind")
              )
            ),
            in_front(
              el(
                [
                  padding(4),
                  Background.color({:color_rgba, {0, 0, 0, 120}}),
                  Border.rounded(4),
                  Font.size(10),
                  Font.color(:white)
                ],
                text("Front")
              )
            )
          ],
          text("Base")
        )
      )
    ])
  end

  defp section_title(label) do
    el([Font.size(16), Font.color(@light_text)], text(label))
  end

  defp menu_item(label, page, current_page, hovered_menu) do
    active = page == current_page
    hovered = page == hovered_menu

    bg =
      cond do
        active -> @blue
        hovered -> {:color_rgb, {70, 70, 100}}
        true -> {:color_rgb, {45, 45, 65}}
      end

    text_color = if active or hovered, do: @light_text, else: @dim_text

    el(
      [
        width(fill()),
        padding(10),
        Background.color(bg),
        Border.rounded(10),
        on_click({self(), {:demo_nav, page}}),
        on_mouse_enter({self(), {:menu_hover, page}}),
        on_mouse_leave({self(), {:menu_hover_clear, page}})
      ],
      el([Font.size(12), Font.color(text_color)], text(label))
    )
  end

  defp drain_events(acc \\ []) do
    receive do
      {:emerge_skia_event, _} = message -> drain_events([message | acc])
      {:demo_event, _, _} = message -> drain_events([message | acc])
      {:demo_nav, _} = message -> drain_events([message | acc])
      {:feature_click, _} = message -> drain_events([message | acc])
      {:menu_hover, _} = message -> drain_events([message | acc])
      {:menu_hover_clear, _} = message -> drain_events([message | acc])
      :scramble_unstable = message -> drain_events([message | acc])
      {:unstable_row_click, _} = message -> drain_events([message | acc])
      {:unstable_child_click, _, _} = message -> drain_events([message | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  defp render_update(renderer, state, tree, {width, height}, scale) do
    {patch_bin, next_state, _assigned} = Emerge.diff_state_update(state, tree)

    case EmergeSkia.Native.renderer_patch(renderer, patch_bin, width, height, scale) do
      :ok -> next_state
      {:ok, _} -> next_state
      {:error, reason} -> raise "renderer_patch failed: #{reason}"
    end
  end

  defp handle_event(
         renderer,
         state,
         mouse_pos,
         event_log,
         size,
         scale,
         current_page,
         hovered_menu,
         last_move_label,
         unstable_items
       ) do
    receive do
      {:emerge_skia_event, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:demo_event, _, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:demo_nav, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:feature_click, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:menu_hover, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:menu_hover_clear, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      :scramble_unstable = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:unstable_row_click, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )

      {:unstable_child_click, _, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
          hovered_menu,
          last_move_label,
          unstable_items
        )
    after
      100 -> :timeout
    end
  end

  defp process_event_batch(
         events,
         renderer,
         state,
         mouse_pos,
         event_log,
         size,
         scale,
         current_page,
         hovered_menu,
         last_move_label,
         unstable_items
       ) do
    {next_state, needs_render} =
      Enum.reduce(
        events,
        {
          {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
           unstable_items},
          false
        },
        fn message, {acc, dirty} ->
          {next_acc, changed} = process_event(message, state, acc)
          {next_acc, dirty or changed}
        end
      )

    {new_mouse_pos, new_log, new_size, new_scale, new_page, new_hovered, new_move, new_unstable} =
      next_state

    if needs_render do
      tree =
        build_tree(
          new_size,
          new_mouse_pos,
          new_log,
          new_page,
          new_hovered,
          new_move,
          new_unstable
        )

      next_state = render_update(renderer, state, tree, new_size, new_scale)

      {:ok, next_state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_hovered,
       new_move, new_unstable}
    else
      {:ok, state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_hovered, new_move,
       new_unstable}
    end
  end

  defp process_event(
         {:emerge_skia_event, event},
         state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
     unstable_items} =
      case event do
        {id_bin, event_type} when is_binary(id_bin) and is_atom(event_type) ->
          case Emerge.lookup_event(state, id_bin, event_type) do
            {:ok, {pid, msg}} when pid == self() ->
              {next_state, _changed} =
                process_event(
                  msg,
                  state,
                  {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
                   unstable_items}
                )

              next_state

            _ ->
              Emerge.dispatch_event(state, id_bin, event_type)

              {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
               unstable_items}
          end

        _ ->
          {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
           unstable_items}
      end

    new_mouse_pos =
      case event do
        {:cursor_pos, {x, y}} -> {x, y}
        {:cursor_button, {_, _, _, {x, y}}} -> {x, y}
        _ -> mouse_pos
      end

    {new_size, new_scale} =
      case event do
        {:resized, {w, h, s}} -> {{w, h}, s}
        _ -> {size, scale}
      end

    new_log =
      case event do
        {:cursor_pos, _} -> event_log
        {_, :mouse_move} -> event_log
        _ -> [format_event(event) | event_log]
      end
      |> Enum.take(20)

    changed =
      new_mouse_pos != mouse_pos or new_log != event_log or new_size != size or
        new_scale != scale

    {{new_mouse_pos, new_log, new_size, new_scale, current_page, hovered_menu, last_move_label,
      unstable_items}, changed}
  end

  defp process_event(
         {:feature_click, title},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    new_log = Enum.take(["UI Click: #{title}" | event_log], 20)

    {{mouse_pos, new_log, size, scale, current_page, hovered_menu, last_move_label,
      unstable_items}, new_log != event_log}
  end

  defp process_event(
         {:demo_nav, page},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    new_log = Enum.take(["Navigate: #{format_page(page)}" | event_log], 20)
    new_page = page
    changed = new_log != event_log or new_page != current_page

    {{mouse_pos, new_log, size, scale, new_page, hovered_menu, last_move_label, unstable_items},
     changed}
  end

  defp process_event(
         {:menu_hover, page},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    new_hovered = page
    changed = new_hovered != hovered_menu

    {{mouse_pos, event_log, size, scale, current_page, new_hovered, last_move_label,
      unstable_items}, changed}
  end

  defp process_event(
         {:menu_hover_clear, page},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    new_hovered = if hovered_menu == page, do: nil, else: hovered_menu
    changed = new_hovered != hovered_menu

    {{mouse_pos, event_log, size, scale, current_page, new_hovered, last_move_label,
      unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, label, event},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    {new_log, new_move_label} =
      case event do
        :mouse_move ->
          if label == last_move_label do
            {event_log, last_move_label}
          else
            entry = "Move: #{label}"
            {Enum.take([entry | event_log], 20), label}
          end

        _ ->
          entry = "#{String.capitalize(format_event_label(event))}: #{label}"
          {Enum.take([entry | event_log], 20), last_move_label}
      end

    changed = new_log != event_log or new_move_label != last_move_label

    {{mouse_pos, new_log, size, scale, current_page, hovered_menu, new_move_label,
      unstable_items}, changed}
  end

  defp process_event(
         :scramble_unstable,
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    {new_items, child_assignments} = scramble_unstable_items(unstable_items)
    new_log = Enum.take(["Scramble: unstable list" | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log or child_assignments > 0

    {{mouse_pos, new_log, size, scale, current_page, hovered_menu, last_move_label, new_items},
     changed}
  end

  defp process_event(
         {:unstable_row_click, label},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    {new_items, next_count} =
      update_unstable_item(unstable_items, label, fn item ->
        updated = %{item | count: item.count + 1}
        {updated, updated.count}
      end)

    new_log = Enum.take(["Unstable row: #{label} (#{next_count})" | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, hovered_menu, last_move_label, new_items},
     changed}
  end

  defp process_event(
         {:unstable_child_click, parent_label, child_label},
         _state,
         {mouse_pos, event_log, size, scale, current_page, hovered_menu, last_move_label,
          unstable_items}
       ) do
    {new_items, child_count} = update_unstable_child(unstable_items, child_label)

    entry = "Unstable child: #{child_label} (#{child_count}) in #{parent_label}"
    new_log = Enum.take([entry | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, hovered_menu, last_move_label, new_items},
     changed}
  end

  defp update_unstable_item(items, label, updater) do
    Enum.map_reduce(items, 0, fn item, acc ->
      if item.label == label do
        {updated, next} = updater.(item)
        {updated, next}
      else
        {item, acc}
      end
    end)
  end

  defp update_unstable_child(items, child_label) do
    Enum.map_reduce(items, 0, fn item, acc ->
      {children, next} =
        Enum.map_reduce(item.children, acc, fn child, acc_count ->
          if child.label == child_label do
            updated = %{child | count: child.count + 1}
            {updated, updated.count}
          else
            {child, acc_count}
          end
        end)

      {%{item | children: children}, next}
    end)
  end

  defp scramble_unstable_items(items) do
    shuffled_parents = Enum.shuffle(items)
    all_children = Enum.flat_map(shuffled_parents, & &1.children)

    {reassigned_children, remainder} =
      shuffled_parents
      |> Enum.reduce({[], all_children}, fn item, {parents, remaining} ->
        {assigned, rest} = take_random_children(remaining, 6, 10)
        {[%{item | children: assigned} | parents], rest}
      end)

    reassigned = Enum.reverse(reassigned_children)

    if remainder == [] do
      {reassigned, length(all_children)}
    else
      {fill_remaining_children(reassigned, remainder), length(all_children)}
    end
  end

  defp take_random_children(children, min_count, max_count) do
    count = min_count + :rand.uniform(max_count - min_count + 1) - 1
    count = min(count, length(children))
    {picked, rest} = Enum.split(Enum.shuffle(children), count)
    {picked, rest}
  end

  defp fill_remaining_children(items, remainder) do
    {filled, rest} =
      Enum.map_reduce(items, remainder, fn item, remaining ->
        if remaining == [] do
          {item, remaining}
        else
          {extra, rest} = Enum.split(remaining, 1)
          {%{item | children: item.children ++ extra}, rest}
        end
      end)

    if rest == [] do
      filled
    else
      fill_remaining_children(filled, rest)
    end
  end

  def run_loop(
        renderer,
        state,
        mouse_pos,
        event_log,
        size,
        scale,
        current_page,
        hovered_menu,
        last_move_label,
        unstable_items
      ) do
    if EmergeSkia.running?(renderer) do
      case handle_event(
             renderer,
             state,
             mouse_pos,
             event_log,
             size,
             scale,
             current_page,
             hovered_menu,
             last_move_label,
             unstable_items
           ) do
        :timeout ->
          run_loop(
            renderer,
            state,
            mouse_pos,
            event_log,
            size,
            scale,
            current_page,
            hovered_menu,
            last_move_label,
            unstable_items
          )

        {:ok, next_state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_hovered,
         new_move_label, new_unstable_items} ->
          run_loop(
            renderer,
            next_state,
            new_mouse_pos,
            new_log,
            new_size,
            new_scale,
            new_page,
            new_hovered,
            new_move_label,
            new_unstable_items
          )
      end
    end
  end

  defp feature_card(title, description, bg_color) do
    column(
      [
        width(fill()),
        on_click({self(), {:feature_click, title}}),
        clip(),
        spacing(8),
        padding(15),
        Background.color(bg_color),
        Border.rounded(8)
      ],
      [
        el([Font.size(16), Font.color(:white)], text(title)),
        el([Font.size(12), Font.color({:color_rgb, {200, 200, 220}})], text(description))
      ]
    )
  end

  defp event_card(label, event, bg_color) do
    el(
      [
        width(fill()),
        padding(14),
        Background.color(bg_color),
        Border.rounded(8),
        event_attr(event, label)
      ],
      column([spacing(6)], [
        el([Font.size(14), Font.color(:white)], text(label)),
        el(
          [Font.size(11), Font.color({:color_rgb, {210, 210, 230}})],
          text("Triggers #{format_event_label(event)}")
        )
      ])
    )
  end

  defp event_attr(:mouse_down, label),
    do: on_mouse_down({self(), {:demo_event, label, :mouse_down}})

  defp event_attr(:mouse_up, label), do: on_mouse_up({self(), {:demo_event, label, :mouse_up}})

  defp event_attr(:mouse_enter, label),
    do: on_mouse_enter({self(), {:demo_event, label, :mouse_enter}})

  defp event_attr(:mouse_leave, label),
    do: on_mouse_leave({self(), {:demo_event, label, :mouse_leave}})

  defp event_attr(:mouse_move, label),
    do: on_mouse_move({self(), {:demo_event, label, :mouse_move}})

  defp format_event_label(event) do
    event
    |> Atom.to_string()
    |> String.replace("_", " ")
  end

  defp chip(label) do
    el(
      [
        padding(6),
        Background.color({:color_rgb, {55, 60, 90}}),
        Border.rounded(12),
        Font.size(11),
        Font.color(:white)
      ],
      text(label)
    )
  end
end

IO.puts("Window opened! Move mouse, click, press keys. Close window to exit.")

initial_size = {800.0, 600.0}
initial_scale = 1.0

initial_unstable_items = [
  %{
    label: "Alpha",
    count: 0,
    children: [
      %{label: "A1", count: 0},
      %{label: "A2", count: 0},
      %{label: "A3", count: 0},
      %{label: "A4", count: 0},
      %{label: "A5", count: 0},
      %{label: "A6", count: 0},
      %{label: "A7", count: 0},
      %{label: "A8", count: 0},
      %{label: "A9", count: 0}
    ]
  },
  %{
    label: "Bravo",
    count: 0,
    children: [
      %{label: "B1", count: 0},
      %{label: "B2", count: 0},
      %{label: "B3", count: 0},
      %{label: "B4", count: 0},
      %{label: "B5", count: 0},
      %{label: "B6", count: 0},
      %{label: "B7", count: 0},
      %{label: "B8", count: 0}
    ]
  },
  %{
    label: "Charlie",
    count: 0,
    children: [
      %{label: "C1", count: 0},
      %{label: "C2", count: 0},
      %{label: "C3", count: 0},
      %{label: "C4", count: 0},
      %{label: "C5", count: 0},
      %{label: "C6", count: 0},
      %{label: "C7", count: 0},
      %{label: "C8", count: 0},
      %{label: "C9", count: 0},
      %{label: "C10", count: 0}
    ]
  },
  %{
    label: "Delta",
    count: 0,
    children: [
      %{label: "D1", count: 0},
      %{label: "D2", count: 0},
      %{label: "D3", count: 0},
      %{label: "D4", count: 0},
      %{label: "D5", count: 0},
      %{label: "D6", count: 0},
      %{label: "D7", count: 0},
      %{label: "D8", count: 0}
    ]
  },
  %{
    label: "Echo",
    count: 0,
    children: [
      %{label: "E1", count: 0},
      %{label: "E2", count: 0},
      %{label: "E3", count: 0},
      %{label: "E4", count: 0},
      %{label: "E5", count: 0},
      %{label: "E6", count: 0},
      %{label: "E7", count: 0},
      %{label: "E8", count: 0}
    ]
  }
]

initial_tree =
  Demo.build_tree(initial_size, {400.0, 300.0}, [], :overview, nil, nil, initial_unstable_items)

state = Emerge.diff_state_new()
{full_bin, state, _assigned} = Emerge.encode_full(state, initial_tree)

case EmergeSkia.Native.renderer_upload(
       renderer,
       full_bin,
       elem(initial_size, 0),
       elem(initial_size, 1),
       initial_scale
     ) do
  :ok -> :ok
  {:ok, _} -> :ok
  {:error, reason} -> raise "renderer_upload failed: #{reason}"
end

Demo.run_loop(
  renderer,
  state,
  {400.0, 300.0},
  [],
  initial_size,
  initial_scale,
  :overview,
  nil,
  nil,
  initial_unstable_items
)

IO.puts("Window closed. Demo complete!")
