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

  def format_event(event) do
    inspect(event)
  end

  def build_tree({_width, _height}, {mx, my}, event_log) do
    column(
      [
        width(:fill),
        height(:fill),
        padding(20),
        spacing(20),
        Background.color(@dark_bg)
      ],
      [
        el(
          [
            padding(16),
            Background.gradient(@blue, @purple, 90),
            Border.rounded(12)
          ],
          el(
            [Emerge.UI.Font.size(26), Emerge.UI.Font.color(@light_text)],
            text("EmergeSkia Demo")
          )
        ),
        row([spacing(20), width(:fill)], [
          el(
            [
              width(:fill),
              padding(16),
              Emerge.UI.Background.color(@event_bg),
              Emerge.UI.Border.rounded(12)
            ],
            column([spacing(6)], [
              el(
                [Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)],
                text("Direct Skia Rendering")
              ),
              el(
                [Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)],
                text("No Scenic overhead")
              ),
              el(
                [Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)],
                text("Minimal NIF surface")
              ),
              el(
                [Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)],
                text("Rust layout + renderer")
              )
            ])
          ),
          el(
            [
              width(:fill),
              padding(16),
              Emerge.UI.Background.color(@event_bg),
              Emerge.UI.Border.rounded(12)
            ],
            column([spacing(6)], [
              el(
                [Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)],
                text("Mouse Position")
              ),
              el(
                [Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)],
                text("X: #{Float.round(mx, 1)}")
              ),
              el(
                [Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)],
                text("Y: #{Float.round(my, 1)}")
              )
            ])
          )
        ]),
        row(
          [width(:fill), height(:fill), spacing(10)],
          [
            column(
              [
                width(:fill),
                padding(16),
                Emerge.UI.Background.color(@event_bg),
                Emerge.UI.Border.rounded(12),
                spacing(8)
              ],
              [
                el(
                  [Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)],
                  text("Input Event Log")
                ),
                column(
                  [spacing(4)],
                  event_log
                  |> Enum.take(16)
                  |> Enum.reverse()
                  |> Enum.map(fn line ->
                    el([Emerge.UI.Font.size(13), Emerge.UI.Font.color(@dim_text)], text(line))
                  end)
                )
              ]
            ),
            main_content()
          ]
        ),
        el(
          [padding(6), Emerge.UI.Background.color(@pink), Emerge.UI.Border.rounded(6)],
          el(
            [Emerge.UI.Font.size(12), Emerge.UI.Font.color(@light_text)],
            text("Cursor: #{Float.round(mx, 1)}, #{Float.round(my, 1)}")
          )
        )
      ]
    )
  end

  def main_content() do
    column(
      [
        width(fill()),
        height(fill()),
        spacing(15),
        padding(20),
        scroll_y(0),
        scrollbar_y(),
        {:id, :main_content},
        Background.color({:color_rgb, {35, 35, 55}}),
        Border.rounded(8)
      ],
      [
        el([Font.size(22), Font.color(:white), {:id, :welcome_title}], text("Welcome to Emerge")),
        el(
          [Font.size(14), Font.color({:color_rgb, {150, 150, 170}})],
          text("An elm-ui inspired layout engine for Scenic")
        ),

        # Feature cards
        row([width(fill()), spacing(15)], [
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

        # Sizing combos
        column([width(fill()), spacing(10)], [
          el([Font.size(14), Font.color({:color_rgb, {190, 190, 210}})], text("Sizing combos")),
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
                el(
                  [Font.size(11), Font.color({:color_rgb, {210, 220, 230}})],
                  text("Content sized")
                )
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
                Border.rounded(8),
                clip()
              ],
              el([Font.size(12), Font.color(:white)], text("Fill 1"))
            ),
            el(
              [
                width(fill_portion(2)),
                padding(8),
                Background.color({:color_rgb, {65, 80, 110}}),
                Border.rounded(8),
                clip()
              ],
              el([Font.size(12), Font.color(:white)], text("Fill 2"))
            ),
            el(
              [
                width(fill_portion(3)),
                padding(8),
                Background.color({:color_rgb, {65, 90, 120}}),
                Border.rounded(8),
                clip()
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
                Border.rounded(8),
                clip()
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
                Border.rounded(8),
                clip()
              ],
              column([spacing(4)], [
                el([Font.size(13), Font.color(:white)], text("Max + fill")),
                el([Font.size(11), Font.color({:color_rgb, {225, 215, 235}})], text("<= 180px"))
              ])
            )
          ])
        ]),

        # Spacing + spaceEvenly
        column([width(fill()), spacing(10)], [
          el(
            [Font.size(14), Font.color({:color_rgb, {190, 190, 210}})],
            text("Spacing + spaceEvenly")
          ),
          row([width(fill()), space_evenly()], [
            chip("Space"),
            chip("Between"),
            chip("Items")
          ]),
          wrapped_row([width(fill()), spacing_xy(16, 18)], [
            chip("Spacing"),
            chip("X/Y"),
            chip("Example"),
            chip("Wrapped"),
            chip("Row")
          ])
        ]),

        # Wrapped row demo
        wrapped_row([width(fill()), spacing(8)], [
          chip("Wrapped"),
          chip("Row"),
          chip("Auto"),
          chip("Line"),
          chip("Breaks"),
          chip("With"),
          chip("Spacing"),
          chip("And"),
          chip("Chips")
        ]),

        # Horizontal scroll demo
        el(
          [
            width(fill()),
            height(px(90)),
            padding(10),
            scroll_x(0),
            scrollbar_x(),
            clip_x(),
            {:id, :horizontal_scroll},
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

        # Nested vertical scroll demo (inside main scroll)
        el(
          [
            width(fill()),
            height(px(120)),
            padding(10),
            scroll_y(0),
            scrollbar_y(),
            clip_y(),
            {:id, :nested_scroll},
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
        ),

        # Alignment demo
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

        # Text alignment inside wider elements
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

        # Centered element with right-aligned text
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

        # Alignment demo
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
        ),
        # Nearby positioning demo
        el(
          [
            width(fill()),
            height(px(140)),
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
      ]
    )
  end

  def drain_mailbox(acc \\ []) do
    receive do
      {:emerge_skia_event, event} -> drain_mailbox([event | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  def process_events(events, state, mouse_pos, event_log, size, scale) do
    Enum.reduce(events, {mouse_pos, event_log, size, scale}, fn event, {pos, log, sz, sc} ->
      case event do
        {id_bin, :click} -> Emerge.dispatch_click(state, id_bin)
        _ -> :ok
      end

      new_pos =
        case event do
          {:cursor_pos, {x, y}} -> {x, y}
          {:cursor_button, {_, _, _, {x, y}}} -> {x, y}
          _ -> pos
        end

      {new_size, new_scale} =
        case event do
          {:resized, {w, h, s}} -> {{w, h}, s}
          _ -> {sz, sc}
        end

      new_log =
        case event do
          {:cursor_pos, _} -> log
          _ -> [format_event(event) | log]
        end

      {new_pos, Enum.take(new_log, 20), new_size, new_scale}
    end)
  end

  def run_loop(renderer, state, mouse_pos, event_log, size, scale) do
    if EmergeSkia.running?(renderer) do
      receive do
        {:emerge_skia_event, first_event} ->
          remaining = drain_mailbox()
          all_events = [first_event | remaining]

          {new_mouse_pos, new_log, new_size, new_scale} =
            process_events(all_events, state, mouse_pos, event_log, size, scale)

          tree = build_tree(new_size, new_mouse_pos, new_log)

          {patch_bin, next_state, _assigned} = Emerge.diff_state_update(state, tree)

          case EmergeSkia.Native.renderer_patch(
                 renderer,
                 patch_bin,
                 elem(new_size, 0),
                 elem(new_size, 1),
                 new_scale
               ) do
            :ok -> :ok
            {:ok, _} -> :ok
            {:error, reason} -> raise "renderer_patch failed: #{reason}"
          end

          run_loop(renderer, next_state, new_mouse_pos, new_log, new_size, new_scale)

        {:feature_click, title} ->
          new_log = Enum.take(["UI Click: #{title}" | event_log], 20)
          tree = build_tree(size, mouse_pos, new_log)

          {patch_bin, next_state, _assigned} = Emerge.diff_state_update(state, tree)

          case EmergeSkia.Native.renderer_patch(
                 renderer,
                 patch_bin,
                 elem(size, 0),
                 elem(size, 1),
                 scale
               ) do
            :ok -> :ok
            {:ok, _} -> :ok
            {:error, reason} -> raise "renderer_patch failed: #{reason}"
          end

          run_loop(renderer, next_state, mouse_pos, new_log, size, scale)
      after
        100 ->
          run_loop(renderer, state, mouse_pos, event_log, size, scale)
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
initial_tree = Demo.build_tree(initial_size, {400.0, 300.0}, [])

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

Demo.run_loop(renderer, state, {400.0, 300.0}, [], initial_size, initial_scale)

IO.puts("Window closed. Demo complete!")
