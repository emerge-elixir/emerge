# Demo script for EmergeSkia
# Run with: mix run demo.exs
# Example DRM: mix run demo.exs -- --backend drm --card /dev/dri/card0 --input-log --render-log

argv =
  case System.argv() do
    ["--" | rest] -> rest
    other -> other
  end

{cli_opts, _rest, _invalid} =
  OptionParser.parse(argv,
    switches: [
      backend: :string,
      card: :string,
      width: :integer,
      height: :integer,
      input_log: :boolean,
      render_log: :boolean
    ],
    aliases: [b: :backend, c: :card, w: :width, h: :height, i: :input_log, r: :render_log]
  )

backend = Keyword.get(cli_opts, :backend, "wayland")
width = Keyword.get(cli_opts, :width, 1920)
height = Keyword.get(cli_opts, :height, 1080)
card = Keyword.get(cli_opts, :card)
input_log = Keyword.get(cli_opts, :input_log, false)
render_log = Keyword.get(cli_opts, :render_log, false)

start_opts = [
  otp_app: :emerge,
  backend: backend,
  title: "EmergeSkia Demo",
  width: width,
  height: height
]

start_opts = if card, do: Keyword.put(start_opts, :drm_card, card), else: start_opts
start_opts = if input_log, do: Keyword.put(start_opts, :input_log, true), else: start_opts
start_opts = if render_log, do: Keyword.put(start_opts, :render_log, true), else: start_opts

startup_detail = if card, do: " backend=#{backend} card=#{card}", else: " backend=#{backend}"
IO.puts("Starting EmergeSkia demo..." <> startup_detail)

blocked_root = Path.join(System.tmp_dir!(), "emerge_skia_demo_blocked")

demo_priv_dir =
  case :code.priv_dir(:emerge) do
    path when is_list(path) -> List.to_string(path)
    _ -> raise "failed to resolve :emerge priv dir"
  end

demo_image_root = Path.join(demo_priv_dir, "demo_images")
demo_font_root = Path.join(demo_priv_dir, "demo_fonts")

required_demo_images = ["static.jpg", "runtime.jpg", "fallback.jpg", "tile_bird_small.jpg"]
required_demo_fonts = ["Lobster-Regular.ttf"]

missing_demo_images =
  Enum.filter(required_demo_images, fn file_name ->
    not File.regular?(Path.join(demo_image_root, file_name))
  end)

if missing_demo_images != [] do
  raise "missing demo image assets in #{demo_image_root}: #{Enum.join(missing_demo_images, ", ")}"
end

missing_demo_fonts =
  Enum.filter(required_demo_fonts, fn file_name ->
    not File.regular?(Path.join(demo_font_root, file_name))
  end)

if missing_demo_fonts != [] do
  raise "missing demo font assets in #{demo_font_root}: #{Enum.join(missing_demo_fonts, ", ")}"
end

File.rm_rf!(blocked_root)
File.mkdir_p!(blocked_root)

runtime_source = Path.join(demo_image_root, "runtime.jpg")
blocked_source = Path.join(blocked_root, "blocked.jpg")
lobster_source = "demo_fonts/Lobster-Regular.ttf"

File.cp!(runtime_source, blocked_source)

start_opts =
  Keyword.put(start_opts, :assets,
    fonts: [
      [
        family: "lobster-demo",
        source: lobster_source,
        weight: 400,
        italic: false
      ]
    ],
    runtime_paths: [
      enabled: true,
      allowlist: [demo_image_root],
      follow_symlinks: false,
      max_file_size: 25_000_000,
      extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
    ]
  )

Process.put(:demo_runtime_allowlist_root, demo_image_root)
Process.put(:demo_runtime_image_path, runtime_source)
Process.put(:demo_restricted_image_path, blocked_source)
Process.put(:demo_font_source, lobster_source)

IO.puts("Configured demo logical source root #{demo_image_root}")
IO.puts("Configured demo font asset #{lobster_source}")

renderer =
  case EmergeSkia.start(start_opts) do
    {:ok, renderer} ->
      renderer

    {:error, reason} ->
      raise "failed to start renderer: #{inspect(reason)}"

    other ->
      raise "unexpected start result: #{inspect(other)}"
  end

EmergeSkia.set_input_target(renderer, self())

Process.put(:log_input, input_log)
Process.put(:log_render, render_log)

demo_pid = self()

clock_now = fn ->
  NaiveDateTime.local_now()
  |> NaiveDateTime.truncate(:second)
  |> Calendar.strftime("%H:%M:%S")
end

Process.put(:clock_time, clock_now.())
Process.put(:hover_manual_active, false)
Process.put(:demo_input_value, "quick brown fox")
Process.put(:demo_input_preedit, nil)
Process.put(:demo_input_preedit_cursor, nil)
Process.put(:demo_input_focused, false)
Process.put(:demo_input_focus_count, 0)
Process.put(:demo_input_blur_count, 0)

clock_loop = fn loop ->
  send(demo_pid, {:clock_tick, clock_now.()})
  Process.sleep(1000)
  loop.(loop)
end

spawn(fn -> clock_loop.(clock_loop) end)

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

  def format_event({:text_commit, {text, mods}}) do
    mods_str = if mods == [], do: "", else: " [#{Enum.join(mods, ", ")}]"
    "Text commit: #{inspect(text)}#{mods_str}"
  end

  def format_event({:text_preedit, {text, cursor}}) do
    "Text preedit: #{inspect(text)} cursor=#{inspect(cursor)}"
  end

  def format_event(:text_preedit_clear) do
    "Text preedit: cleared"
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
        last_move_label,
        unstable_items
      ) do
    render_seq = Process.get(:render_seq, 0)
    log_render = Process.get(:log_render, false)
    clock_time = Process.get(:clock_time, "--:--:--")

    render_rows =
      if log_render do
        [el([Font.size(12), Font.color(@dim_text)], text("Render: #{render_seq}"))]
      else
        []
      end

    clock_row = el([Font.size(12), Font.color(@dim_text)], text("Clock: #{clock_time}"))

    column(
      [
        width(:fill),
        height(:fill),
        padding(20),
        spacing(16),
        Background.color(@dark_bg)
      ],
      [
        header_section(mx, my, [clock_row | render_rows]),
        row([width(:fill), height(:fill), spacing(16)], [
          menu_panel(current_page),
          content_panel(current_page, last_move_label, unstable_items)
        ]),
        footer_bar(mx, my, event_log)
      ]
    )
  end

  defp header_section(mx, my, render_rows) do
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
        column(
          [spacing(4)],
          [
            el([Font.size(14), Font.color(@light_text)], text("Live Input")),
            el([Font.size(12), Font.color(@dim_text)], text("X: #{Float.round(mx, 1)}")),
            el([Font.size(12), Font.color(@dim_text)], text("Y: #{Float.round(my, 1)}"))
          ] ++ render_rows
        )
      )
    ])
  end

  defp menu_panel(current_page) do
    menu_items = [
      {"Overview", :overview},
      {"Layout", :layout},
      {"Scroll", :scroll},
      {"Alignment", :alignment},
      {"Transforms", :transforms},
      {"Events", :events},
      {"Hover", :hover},
      {"Unstable List", :unstable_list},
      {"Nearby", :nearby},
      {"Text", :text},
      {"Inupt", :inupt},
      {"Borders", :borders},
      {"Assets", :assets}
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
            menu_item(label, page, current_page)
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
      :hover -> page_hover()
      :unstable_list -> page_unstable_list(unstable_items)
      :nearby -> page_nearby()
      :text -> page_text()
      :inupt -> page_inupt()
      :borders -> page_borders()
      :assets -> page_assets()
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

  defp page_assets() do
    runtime_path = Process.get(:demo_runtime_image_path, "runtime.jpg")
    static_source = "demo_images/static.jpg"
    bird_tile_source = "demo_images/tile_bird_small.jpg"
    font_source = Process.get(:demo_font_source, "demo_fonts/Lobster-Regular.ttf")

    restricted_path =
      Process.get(:demo_restricted_image_path, "/tmp/emerge_skia_demo_blocked/blocked.jpg")

    runtime_allowlist_root = Process.get(:demo_runtime_allowlist_root, "(unknown)")

    fit_frames = [
      {"Wide frame", {280, 120}},
      {"Tall frame", {140, 240}},
      {"Square frame", {180, 180}}
    ]

    image_fit_cards =
      Enum.flat_map(fit_frames, fn {label, {frame_w, frame_h}} ->
        Enum.map([:contain, :cover], fn fit ->
          fit_demo_card(
            "image/2",
            label,
            {frame_w, frame_h},
            fit,
            :element,
            static_source
          )
        end)
      end)

    background_fit_cards =
      Enum.flat_map(fit_frames, fn {label, {frame_w, frame_h}} ->
        Enum.map([:contain, :cover], fn fit ->
          fit_demo_card(
            "Background.image/2",
            label,
            {frame_w, frame_h},
            fit,
            :background,
            static_source
          )
        end)
      end)

    source_cards =
      [
        %{
          title: "Static source",
          source_label: ~s(source: "demo_images/static.jpg"),
          status: {"Source root", :source},
          preview: {:image, static_source, :contain, "image :contain"}
        },
        %{
          title: "Runtime source",
          source_label: "source: {:path, runtime.jpg}",
          status: {"Allowlisted", :runtime},
          preview: {:image, {:path, runtime_path}, :cover, "image :cover"}
        },
        %{
          title: "Restricted source",
          source_label: "source outside allowlist",
          status: {"Blocked", :blocked},
          preview: {:image, {:path, restricted_path}, :contain, "blocked"}
        }
      ]
      |> Enum.map(&asset_behavior_card/1)

    font_cards =
      [
        %{
          title: "Default Inter",
          source_label: "source: built-in default",
          status: {"Built-in", :font_builtin},
          note: "No assets.fonts entry required",
          attrs: []
        },
        %{
          title: "Lobster asset",
          source_label: ~s(source: "#{font_source}"),
          status: {"Font asset", :font},
          note: "Loaded at startup from otp_app priv",
          attrs: [Font.family("lobster-demo")]
        },
        %{
          title: "Lobster synthetic bold + italic",
          source_label: ~s(source: "#{font_source}"),
          status: {"Synthetic", :synthetic},
          note: "Only regular is loaded, style is synthesized",
          attrs: [Font.family("lobster-demo"), Font.bold(), Font.italic()]
        }
      ]
      |> Enum.map(&font_asset_card/1)

    background_cards =
      [
        %{
          title: "Background.image/1",
          source_label: "source: demo_images/tile_bird_small.jpg",
          status: {"Background", :background},
          preview: {:background, Background.image(bird_tile_source), "bg :cover"}
        },
        %{
          title: "Background.uncropped/1",
          source_label: "source: demo_images/tile_bird_small.jpg",
          status: {"Helper", :helper},
          preview: {:background, Background.uncropped(bird_tile_source), "bg :contain"}
        },
        %{
          title: "Background.tiled/1",
          source_label: "source: demo_images/tile_bird_small.jpg",
          status: {"Helper", :helper},
          preview: {:background, Background.tiled(bird_tile_source), "bg :repeat"}
        },
        %{
          title: "Background.tiled_x/1",
          source_label: "source: demo_images/tile_bird_small.jpg",
          status: {"Helper", :helper},
          preview: {:background, Background.tiled_x(bird_tile_source), "bg :repeat_x"}
        },
        %{
          title: "Background.tiled_y/1",
          source_label: "source: demo_images/tile_bird_small.jpg",
          status: {"Helper", :helper},
          preview: {:background, Background.tiled_y(bird_tile_source), "bg :repeat_y"}
        }
      ]
      |> Enum.map(&asset_behavior_card/1)

    column([width(fill()), spacing(16)], [
      section_title("Assets"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Assets resolve from otp_app priv or runtime paths, then render through image/2, Background helpers, and startup-loaded font assets."
        )
      ),
      section_title("Source"),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text("How each source type resolves before rendering.")
      ),
      centered_wrapped_cards(source_cards, 936),
      section_title("Fonts"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Startup font assets are resolved from priv and mapped by family/weight/style.")
      ),
      centered_wrapped_cards(font_cards, 936),
      section_title("Image"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("image/2 fit behavior with the same source across different frame ratios.")
      ),
      centered_wrapped_cards(image_fit_cards, 960),
      fit_legend(),
      section_title("Background"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Background.image plus helper variants. Same card design, only background attributes differ."
        )
      ),
      centered_wrapped_cards(background_cards, 936),
      el(
        [Font.size(10), Font.color(@dim_text)],
        text("Tile source: demo_images/tile_bird_small.jpg (160x120)")
      ),
      el(
        [Font.size(12), Font.color({:color_rgb, {205, 214, 229}})],
        text("Background.image/2 fit behavior")
      ),
      centered_wrapped_cards(background_fit_cards, 960),
      el(
        [
          width(fill()),
          padding(10),
          spacing(6),
          Background.color({:color_rgb, {48, 48, 72}}),
          Border.rounded(8)
        ],
        column([spacing(4)], [
          el([Font.size(12), Font.color(:white)], text("Demo policy")),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text("async asset loading + loading/failed placeholders")
          ),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text("runtime allowlist: #{runtime_allowlist_root}")
          ),
          el([Font.size(11), Font.color(@dim_text)], text("font asset: #{font_source}"))
        ])
      )
    ])
  end

  defp page_inupt() do
    value = Process.get(:demo_input_value, "quick brown fox")
    preedit = Process.get(:demo_input_preedit, nil)
    preedit_cursor = Process.get(:demo_input_preedit_cursor, nil)
    focused = Process.get(:demo_input_focused, false)
    focus_count = Process.get(:demo_input_focus_count, 0)
    blur_count = Process.get(:demo_input_blur_count, 0)

    {status_label, status_bg, status_text, input_border_color, input_border_width} =
      if focused do
        {
          "Focused",
          {:color_rgb, {72, 96, 70}},
          {:color_rgb, {227, 244, 223}},
          {:color_rgb, {228, 183, 104}},
          1
        }
      else
        {
          "Blurred",
          {:color_rgb, {72, 74, 102}},
          {:color_rgb, {220, 224, 240}},
          {:color_rgb, {120, 130, 175}},
          1
        }
      end

    value_label =
      case value do
        "" -> "(empty)"
        _ -> value
      end

    preedit_label =
      case preedit do
        nil -> "(none)"
        "" -> "(empty)"
        _ -> preedit
      end

    column([width(fill()), spacing(16)], [
      section_title("Inupt"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Text input: click/drag to select, shift+arrows, ctrl/meta+a/c/x/v, middle-click paste, backspace/delete."
        )
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color({:color_rgb, {48, 48, 72}}),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          Emerge.UI.Input.text(value, [
            width(fill()),
            padding_xy(10, 8),
            Font.size(16),
            Font.color(:white),
            Background.color({:color_rgb, {62, 62, 94}}),
            Border.rounded(8),
            Border.width(input_border_width),
            Border.color(input_border_color),
            on_change({self(), {:demo_event, :inupt_changed}}),
            on_focus({self(), {:demo_event, :inupt_focus}}),
            on_blur({self(), {:demo_event, :inupt_blur}})
          ]),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text(
              "on_change emits each edit from Rust; on_focus/on_blur fire on focus transitions."
            )
          ),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            el(
              [
                padding_xy(10, 5),
                Background.color(status_bg),
                Border.rounded(999)
              ],
              el([Font.size(11), Font.color(status_text)], text("State: #{status_label}"))
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color({:color_rgb, {64, 74, 106}}),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color({:color_rgb, {205, 216, 246}})],
                text("focus: #{focus_count}")
              )
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color({:color_rgb, {78, 68, 100}}),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color({:color_rgb, {228, 212, 246}})],
                text("blur: #{blur_count}")
              )
            )
          ]),
          el(
            [Font.size(12), Font.color({:color_rgb, {225, 228, 244}})],
            text("Value: #{value_label}")
          ),
          el([Font.size(11), Font.color(@dim_text)], text("Length: #{String.length(value)}")),
          el([Font.size(11), Font.color(@dim_text)], text("Preedit: #{preedit_label}")),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text("Preedit cursor: #{inspect(preedit_cursor)}")
          )
        ])
      )
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

  defp page_hover() do
    manual_active = Process.get(:hover_manual_active, false)

    column([width(fill()), spacing(16)], [
      section_title("Hover"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Compare event-driven hover state with declarative mouse_over styling.")
      ),
      row([width(fill()), spacing(16)], [
        hover_showcase_event_panel(manual_active),
        hover_showcase_mouse_over_panel()
      ])
    ])
  end

  defp hover_showcase_event_panel(active) do
    bg = if active, do: {:color_rgb, {88, 72, 122}}, else: {:color_rgb, {58, 52, 82}}
    border = if active, do: {:color_rgb, {188, 154, 250}}, else: {:color_rgb, {120, 112, 150}}
    title_color = if active, do: @light_text, else: {:color_rgb, {220, 210, 240}}
    state_text = if active, do: "state: hovered", else: "state: idle"

    column([width(fill()), spacing(10)], [
      el([Font.size(14), Font.color(@light_text)], text("on_mouse_enter / on_mouse_leave")),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text("Hover events are sent to Elixir, which toggles local state and re-renders styles.")
      ),
      el(
        [
          width(fill()),
          padding(14),
          Background.color(bg),
          Border.rounded(10),
          Border.width(1),
          Border.color(border),
          move_y(if(active, do: -2, else: 0)),
          on_mouse_enter({self(), {:demo_event, :hover_manual, :mouse_enter}}),
          on_mouse_leave({self(), {:demo_event, :hover_manual, :mouse_leave}})
        ],
        column([spacing(6)], [
          el([Font.size(13), Font.color(title_color)], text("Event-managed hover")),
          el([Font.size(11), Font.color(title_color)], text(state_text)),
          el(
            [Font.size(10), Font.color({:color_rgb, {225, 215, 245}})],
            text("Behavior is explicit and can trigger arbitrary app logic.")
          )
        ])
      )
    ])
  end

  defp hover_showcase_mouse_over_panel() do
    column([width(fill()), spacing(10)], [
      el([Font.size(14), Font.color(@light_text)], text("mouse_over")),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text("Styles live on the element. Rust tracks hover and applies decorative attrs.")
      ),
      el(
        [
          width(fill()),
          padding(14),
          Background.color({:color_rgb, {52, 70, 84}}),
          Border.rounded(10),
          Border.width(1),
          Border.color({:color_rgb, {102, 124, 150}}),
          mouse_over([
            Background.color({:color_rgb, {86, 112, 140}}),
            Border.color({:color_rgb, {168, 210, 250}}),
            Font.color(@light_text),
            Font.underline(),
            Font.strike(),
            Font.letter_spacing(1.4),
            Font.word_spacing(2.5),
            move_y(-2),
            scale(1.02)
          ])
        ],
        column([spacing(6)], [
          el(
            [Font.size(13), Font.color({:color_rgb, {210, 222, 240}})],
            text("Declarative hover style")
          ),
          el(
            [Font.size(11), Font.color({:color_rgb, {190, 206, 228}})],
            text("No enter/leave handlers or hover state in Elixir.")
          ),
          el(
            [Font.size(10), Font.color({:color_rgb, {214, 228, 246}})],
            text("This hover also toggles underline/strike and letter/word spacing.")
          )
        ])
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

  defp page_text() do
    column([width(fill()), spacing(16)], [
      section_title("Font Inheritance"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Fonts set on containers propagate to all text children")
      ),
      column(
        [
          width(fill()),
          padding(12),
          spacing(8),
          Font.size(14),
          Font.color({:color_rgb, {200, 220, 255}}),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(8)
        ],
        [
          text("This text inherits font from column"),
          text("No Font.size() or Font.color() here"),
          row([spacing(8)], [
            text("Row child 1"),
            text("Row child 2"),
            text("All inherited")
          ])
        ]
      ),
      section_title("Font Sizes"),
      row([width(fill()), spacing(12), align_bottom()], [
        el([Font.size(10), Font.color(:white)], text("10px")),
        el([Font.size(12), Font.color(:white)], text("12px")),
        el([Font.size(14), Font.color(:white)], text("14px")),
        el([Font.size(16), Font.color(:white)], text("16px")),
        el([Font.size(20), Font.color(:white)], text("20px")),
        el([Font.size(24), Font.color(:white)], text("24px"))
      ]),
      section_title("Font Weight & Style"),
      row([width(fill()), spacing(16)], [
        el([Font.size(14), Font.color(:white)], text("Normal")),
        el([Font.size(14), Font.color(:white), Font.bold()], text("Bold")),
        el([Font.size(14), Font.color(:white), Font.italic()], text("Italic")),
        el([Font.size(14), Font.color(:white), Font.bold(), Font.italic()], text("Bold Italic"))
      ]),
      section_title("Text Decoration & Spacing"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Underline/strike and letter/word spacing can be inherited or applied per element")
      ),
      row([width(fill()), spacing(10)], [
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {54, 70, 90}}),
            Border.rounded(6),
            Font.size(13),
            Font.color(:white),
            Font.underline()
          ],
          text("Underline")
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {72, 62, 88}}),
            Border.rounded(6),
            Font.size(13),
            Font.color(:white),
            Font.strike()
          ],
          text("Strike")
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {70, 80, 62}}),
            Border.rounded(6),
            Font.size(13),
            Font.color(:white),
            Font.underline(),
            Font.strike()
          ],
          text("Underline + Strike")
        )
      ]),
      row([width(fill()), spacing(10)], [
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {45, 60, 82}}),
            Border.rounded(6),
            Font.size(12),
            Font.color(:white),
            Font.letter_spacing(2.5)
          ],
          text("LETTER SPACING")
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {56, 74, 66}}),
            Border.rounded(6),
            Font.size(12),
            Font.color(:white),
            Font.word_spacing(5)
          ],
          text("word spacing demo")
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {75, 62, 62}}),
            Border.rounded(6),
            Font.size(12),
            Font.color(:white),
            Font.underline(),
            Font.letter_spacing(1.5),
            Font.word_spacing(3)
          ],
          text("combined spacing")
        )
      ]),
      section_title("Text Alignment"),
      row([width(fill()), spacing(8)], [
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4)
          ],
          el([width(fill()), Font.size(12), Font.color(:white), Font.align_left()], text("Left"))
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4)
          ],
          el([width(fill()), Font.size(12), Font.color(:white), Font.center()], text("Center"))
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color({:color_rgb, {55, 55, 80}}),
            Border.rounded(4)
          ],
          el(
            [width(fill()), Font.size(12), Font.color(:white), Font.align_right()],
            text("Right")
          )
        )
      ]),
      section_title("Inheritance Override"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Child elements can override inherited font settings")
      ),
      column(
        [
          width(fill()),
          padding(12),
          spacing(8),
          Font.size(14),
          Font.color({:color_rgb, {180, 180, 200}}),
          Background.color({:color_rgb, {50, 50, 70}}),
          Border.rounded(8)
        ],
        [
          text("Inherited: 14px, gray"),
          el([Font.size(18), Font.color(:cyan)], text("Override: 18px, cyan")),
          el([Font.bold()], text("Override: bold only")),
          text("Back to inherited")
        ]
      ),
      section_title("Paragraph"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Paragraph elements wrap text automatically within a constrained width")
      ),
      el(
        [
          width(px(400)),
          padding(12),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(8)
        ],
        paragraph([Font.size(14), Font.color(:white)], [
          text(
            "This is a paragraph that demonstrates automatic word wrapping. " <>
              "When text exceeds the available width, it flows naturally to the next line, " <>
              "just like in a word processor or web browser. No manual line breaks needed."
          )
        ])
      ),
      section_title("Inline Styled Spans"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Mix plain text with bold and colored spans inside a paragraph")
      ),
      el(
        [
          width(px(450)),
          padding(12),
          Background.color({:color_rgb, {45, 45, 65}}),
          Border.rounded(8)
        ],
        paragraph([Font.size(14), Font.color(:white)], [
          text("Paragraphs support "),
          el([Font.bold()], text("bold text")),
          text(" and "),
          el([Font.color(@pink)], text("colored spans")),
          text(" inline. This lets you build rich text layouts where "),
          el([Font.bold(), Font.color(@blue)], text("styled fragments")),
          text(" flow naturally within the same line-wrapped block.")
        ])
      ),
      section_title("Line Spacing"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Compare tight vs relaxed line spacing in paragraphs")
      ),
      row([width(fill()), spacing(16)], [
        column([width(fill()), spacing(6)], [
          el([Font.size(11), Font.color(@dim_text)], text("spacing(0)")),
          el(
            [
              width(fill()),
              padding(10),
              Background.color({:color_rgb, {45, 45, 65}}),
              Border.rounded(6)
            ],
            paragraph([spacing(0), Font.size(13), Font.color(:white)], [
              text(
                "Tight line spacing makes text feel compact and dense. " <>
                  "Good for code-like displays or space-constrained layouts."
              )
            ])
          )
        ]),
        column([width(fill()), spacing(6)], [
          el([Font.size(11), Font.color(@dim_text)], text("spacing(8)")),
          el(
            [
              width(fill()),
              padding(10),
              Background.color({:color_rgb, {45, 45, 65}}),
              Border.rounded(6)
            ],
            paragraph([spacing(8), Font.size(13), Font.color(:white)], [
              text(
                "Relaxed line spacing improves readability for body text. " <>
                  "Good for articles, documentation, and longer content."
              )
            ])
          )
        ])
      ]),
      section_title("Document Style"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Heading plus body paragraphs forming a realistic document layout")
      ),
      el(
        [
          width(fill()),
          padding(16),
          Background.color({:color_rgb, {40, 40, 60}}),
          Border.rounded(10)
        ],
        column([width(fill()), spacing(12)], [
          el([Font.size(20), Font.bold(), Font.color(:white)], text("Getting Started")),
          paragraph([spacing(4), Font.size(14), Font.color({:color_rgb, {210, 210, 230}})], [
            text(
              "Emerge is a native GUI toolkit for Elixir that renders with Skia. " <>
                "It uses a declarative layout model inspired by elm-ui, where you describe " <>
                "what your interface should look like and the engine handles the rest."
            )
          ]),
          paragraph([spacing(4), Font.size(14), Font.color({:color_rgb, {210, 210, 230}})], [
            text("To get started, add "),
            el([Font.bold(), Font.color(@blue)], text("emerge_skia")),
            text(" to your dependencies and call "),
            el([Font.bold(), Font.color(@blue)], text("EmergeSkia.start/1")),
            text(
              ". From there you can build your UI tree using the helpers in Emerge.UI " <>
                "and send it to the renderer."
            )
          ])
        ])
      ),
      section_title("Text Column"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Use text_column to group multiple paragraphs into a readable article block")
      ),
      el(
        [
          width(fill()),
          padding(16),
          Background.color({:color_rgb, {37, 44, 58}}),
          Border.rounded(10)
        ],
        text_column(
          [
            center_x(),
            spacing(14),
            Font.size(14),
            Font.color({:color_rgb, {220, 226, 236}})
          ],
          [
            paragraph([spacing(4)], [
              text(
                "Text columns are useful for blog posts, release notes, and long-form product " <>
                  "explanations where several paragraphs should read as one section."
              )
            ]),
            paragraph([spacing(4)], [
              text("This block fills the available width by default, and you can still "),
              el([Font.bold(), Font.color(@blue)], text("override width or spacing")),
              text(" when a specific layout needs tighter control.")
            ]),
            paragraph([spacing(4)], [
              text(
                "In this demo it behaves like a vertical text container with paragraph-friendly " <>
                  "defaults, so it is easy to compose document-style content."
              )
            ])
          ]
        )
      ),
      section_title("Float Flow"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("align_left and align_right float blocks inside paragraph and text_column content")
      ),
      el(
        [
          width(fill()),
          padding(16),
          Background.color({:color_rgb, {44, 50, 66}}),
          Border.rounded(10)
        ],
        paragraph([spacing(4), Font.size(14), Font.color({:color_rgb, {226, 232, 243}})], [
          el(
            [
              align_left(),
              width(px(40)),
              height(px(40)),
              Background.color({:color_rgb, {74, 113, 214}}),
              Border.rounded(8)
            ],
            el(
              [
                Font.bold(),
                Font.size(26),
                Font.color(:white),
                center_x(),
                center_y()
              ],
              text("S")
            )
          ),
          text(
            "tylish copy can wrap around a left-floated drop cap. This paragraph demonstrates " <>
              "inline flow with richer composition while keeping word wrapping automatic. "
          ),
          el(
            [
              align_right(),
              width(px(96)),
              padding(8),
              Background.color({:color_rgb, {78, 58, 90}}),
              Border.rounded(6),
              Font.size(11),
              Font.bold(),
              Font.color(:white)
            ],
            text("PULL QUOTE")
          ),
          text(
            "A right float can sit beside the same text flow, and once the floated blocks end, " <>
              "text returns to full width for the rest of the paragraph."
          )
        ])
      ),
      el(
        [
          width(fill()),
          padding(16),
          Background.color({:color_rgb, {36, 46, 60}}),
          Border.rounded(10)
        ],
        text_column(
          [spacing(10), Font.size(13), Font.color({:color_rgb, {220, 228, 238}})],
          [
            el(
              [
                align_left(),
                width(px(128)),
                height(px(92)),
                padding(10),
                Background.color({:color_rgb, {67, 97, 150}}),
                Border.rounded(8)
              ],
              column([spacing(6)], [
                el([Font.bold(), Font.color(:white)], text("Floated Card")),
                el(
                  [Font.size(11), Font.color({:color_rgb, {232, 238, 246}})],
                  text("align_left()")
                ),
                el([Font.size(11), Font.color({:color_rgb, {232, 238, 246}})], text("92px tall"))
              ])
            ),
            paragraph([spacing(3)], [
              text(
                "This paragraph wraps around the floated card first, using the remaining line width " <>
                  "to the right."
              )
            ]),
            paragraph([spacing(3)], [
              text(
                "A second paragraph keeps flowing around the same active float until its bottom edge " <>
                  "is passed."
              )
            ]),
            el(
              [
                width(fill()),
                padding(8),
                Background.color({:color_rgb, {84, 62, 62}}),
                Border.rounded(6),
                Font.size(12),
                Font.color(:white)
              ],
              text("Non-paragraph block: clears below active floats before rendering")
            ),
            paragraph([spacing(3)], [
              text(
                "After the clear block, flow continues as normal content in the text column with " <>
                  "consistent vertical spacing."
              )
            ])
          ]
        )
      )
    ])
  end

  defp page_borders() do
    column([width(fill()), spacing(16)], [
      section_title("Border Styles"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Style, radius, and width permutations for solid/dashed/dotted borders")
      ),
      border_showcase_grid(border_style_cards()),
      section_title("Per-Edge Border Width"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Per-edge style matrix plus top/right/bottom/left-only permutations")
      ),
      border_showcase_grid(per_edge_border_cards()),
      section_title("Box Shadow"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Directional, spread, and stacked drop shadow permutations")
      ),
      border_showcase_grid(box_shadow_cards()),
      section_title("Glow"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Glow color and intensity permutations")
      ),
      border_showcase_grid(glow_cards()),
      section_title("Inner Shadow"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Inset shadow direction and strength permutations")
      ),
      border_showcase_grid(inner_shadow_cards()),
      section_title("Combined"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Multi-attribute recipes combining border, shadow, glow, and inset variants")
      ),
      border_showcase_grid(combined_border_cards())
    ])
  end

  defp border_style_cards() do
    [
      border_card_spec("Solid thin round", "1px stroke + small radius", [
        border_attr("Border.rounded(6)", Border.rounded(6)),
        border_attr("Border.width(1)", Border.width(1)),
        border_attr("Border.color(:blue)", Border.color(:blue)),
        border_attr("Border.solid()", Border.solid())
      ]),
      border_card_spec("Solid thick square", "5px stroke + square corners", [
        border_attr("Border.rounded(0)", Border.rounded(0)),
        border_attr("Border.width(5)", Border.width(5)),
        border_attr("Border.color(:teal)", Border.color(:teal)),
        border_attr("Border.solid()", Border.solid())
      ]),
      border_card_spec("Dashed medium round", "2px dashes + 8px radius", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.width(2)", Border.width(2)),
        border_attr("Border.color(:orange)", Border.color(:orange)),
        border_attr("Border.dashed()", Border.dashed())
      ]),
      border_card_spec("Dashed thick pill", "4px dashes + large radius", [
        border_attr("Border.rounded(14)", Border.rounded(14)),
        border_attr("Border.width(4)", Border.width(4)),
        border_attr("Border.color(:yellow)", Border.color(:yellow)),
        border_attr("Border.dashed()", Border.dashed())
      ]),
      border_card_spec("Dotted medium round", "2px dots + 10px radius", [
        border_attr("Border.rounded(10)", Border.rounded(10)),
        border_attr("Border.width(2)", Border.width(2)),
        border_attr("Border.color(:magenta)", Border.color(:magenta)),
        border_attr("Border.dotted()", Border.dotted())
      ]),
      border_card_spec("Dotted thick square", "4px dots + no radius", [
        border_attr("Border.rounded(0)", Border.rounded(0)),
        border_attr("Border.width(4)", Border.width(4)),
        border_attr("Border.color(:pink)", Border.color(:pink)),
        border_attr("Border.dotted()", Border.dotted())
      ])
    ]
  end

  defp per_edge_border_cards() do
    style_cards =
      Enum.map(border_style_variants(), fn variant ->
        border_card_spec("Thick top/bottom (#{variant.name})", "width_each(4, 1, 4, 1)", [
          border_attr("Border.rounded(8)", Border.rounded(8)),
          border_attr("Border.width_each(4, 1, 4, 1)", Border.width_each(4, 1, 4, 1)),
          border_attr("Border.color(:#{variant.color})", Border.color(variant.color)),
          border_attr(variant.style_label, variant.style_attr)
        ])
      end)

    single_side_cards =
      for variant <- border_style_variants(),
          {label, top, right, bottom, left} <- single_side_widths() do
        border_card_spec("#{label} (#{variant.name})", "Single-side width_each variant", [
          border_attr("Border.rounded(8)", Border.rounded(8)),
          border_attr(
            "Border.width_each(#{top}, #{right}, #{bottom}, #{left})",
            Border.width_each(top, right, bottom, left)
          ),
          border_attr("Border.color(:#{variant.color})", Border.color(variant.color)),
          border_attr(variant.style_label, variant.style_attr)
        ])
      end

    style_cards ++ single_side_cards
  end

  defp box_shadow_cards() do
    [
      border_card_spec("Drop shadow down-right", "Classic card shadow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {4, 4}, blur: 12, color: :black)",
          Border.shadow(offset: {4, 4}, blur: 12, color: :black)
        )
      ]),
      border_card_spec("Lifted up-left", "Negative offset variation", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {-4, -4}, blur: 12, color: :black)",
          Border.shadow(offset: {-4, -4}, blur: 12, color: :black)
        )
      ]),
      border_card_spec("Right cast", "Directional horizontal shadow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {8, 0}, blur: 10, size: 1, color: :navy)",
          Border.shadow(offset: {8, 0}, blur: 10, size: 1, color: :navy)
        )
      ]),
      border_card_spec("Bottom cast", "Directional vertical shadow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {0, 8}, blur: 10, size: 1, color: :purple)",
          Border.shadow(offset: {0, 8}, blur: 10, size: 1, color: :purple)
        )
      ]),
      border_card_spec("Diffuse spread", "Large blur + spread size", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {0, 0}, blur: 20, size: 2, color: :blue)",
          Border.shadow(offset: {0, 0}, blur: 20, size: 2, color: :blue)
        )
      ]),
      border_card_spec("Stacked shadows", "Two shadow layers on one element", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.shadow(offset: {2, 2}, blur: 6, color: :black)",
          Border.shadow(offset: {2, 2}, blur: 6, color: :black)
        ),
        border_attr(
          "Border.shadow(offset: {-2, -2}, blur: 8, color: :cyan)",
          Border.shadow(offset: {-2, -2}, blur: 8, color: :cyan)
        )
      ])
    ]
  end

  defp glow_cards() do
    [
      border_card_spec("Cyan soft", "Low intensity glow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:cyan, 2)", Border.glow(:cyan, 2))
      ]),
      border_card_spec("Cyan medium", "Balanced glow size", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:cyan, 4)", Border.glow(:cyan, 4))
      ]),
      border_card_spec("Pink strong", "High intensity glow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:pink, 7)", Border.glow(:pink, 7))
      ]),
      border_card_spec("Blue medium", "Cool-toned glow", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:blue, 5)", Border.glow(:blue, 5))
      ]),
      border_card_spec("Purple soft", "Subtle colored rim", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:purple, 3)", Border.glow(:purple, 3))
      ]),
      border_card_spec("Green medium", "Alternative accent color", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr("Border.glow(:green, 4)", Border.glow(:green, 4))
      ])
    ]
  end

  defp inner_shadow_cards() do
    [
      border_card_spec("Inset neutral", "Centered inner depth", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(blur: 10, color: :black)",
          Border.inner_shadow(blur: 10, color: :black)
        )
      ]),
      border_card_spec("Inset strong", "Larger blur and spread", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(blur: 16, size: 2, color: :black)",
          Border.inner_shadow(blur: 16, size: 2, color: :black)
        )
      ]),
      border_card_spec("Inset down-right", "Directional pressed effect", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(offset: {3, 3}, blur: 8, color: :purple)",
          Border.inner_shadow(offset: {3, 3}, blur: 8, color: :purple)
        )
      ]),
      border_card_spec("Inset up-left", "Reverse directional inset", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(offset: {-3, -3}, blur: 8, color: :purple)",
          Border.inner_shadow(offset: {-3, -3}, blur: 8, color: :purple)
        )
      ]),
      border_card_spec("Inset cyan tint", "Colored inner contour", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(blur: 10, color: :cyan)",
          Border.inner_shadow(blur: 10, color: :cyan)
        )
      ]),
      border_card_spec("Inset pink tint", "Warm inner contour", [
        border_attr("Border.rounded(8)", Border.rounded(8)),
        border_attr(
          "Border.inner_shadow(blur: 12, color: :pink)",
          Border.inner_shadow(blur: 12, color: :pink)
        )
      ])
    ]
  end

  defp combined_border_cards() do
    [
      border_card_spec(
        "Dashed + drop shadow",
        "Classic outlined card with depth",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width(2)", Border.width(2)),
          border_attr("Border.color(:purple)", Border.color(:purple)),
          border_attr("Border.dashed()", Border.dashed()),
          border_attr(
            "Border.shadow(offset: {3, 3}, blur: 10, color: :black)",
            Border.shadow(offset: {3, 3}, blur: 10, color: :black)
          )
        ]
      ),
      border_card_spec(
        "Dotted + glow + inset",
        "Outer energy + inner depth",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width(2)", Border.width(2)),
          border_attr("Border.color(:pink)", Border.color(:pink)),
          border_attr("Border.dotted()", Border.dotted()),
          border_attr("Border.glow(:cyan, 3)", Border.glow(:cyan, 3)),
          border_attr(
            "Border.inner_shadow(blur: 8, color: :black)",
            Border.inner_shadow(blur: 8, color: :black)
          )
        ],
        [Background.gradient(@blue, @purple, 135)]
      ),
      border_card_spec(
        "Per-edge solid + shadow",
        "Asymmetric border plus depth",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width_each(4, 1, 4, 1)", Border.width_each(4, 1, 4, 1)),
          border_attr("Border.color(:teal)", Border.color(:teal)),
          border_attr("Border.solid()", Border.solid()),
          border_attr(
            "Border.shadow(offset: {2, 2}, blur: 8, color: :black)",
            Border.shadow(offset: {2, 2}, blur: 8, color: :black)
          )
        ]
      ),
      border_card_spec(
        "Per-edge dashed + glow",
        "Directional widths with luminous edge",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width_each(0, 3, 3, 1)", Border.width_each(0, 3, 3, 1)),
          border_attr("Border.color(:orange)", Border.color(:orange)),
          border_attr("Border.dashed()", Border.dashed()),
          border_attr("Border.glow(:blue, 4)", Border.glow(:blue, 4))
        ]
      ),
      border_card_spec(
        "Solid + stacked shadows",
        "Dual outer layers with crisp border",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width(2)", Border.width(2)),
          border_attr("Border.color(:white)", Border.color(:white)),
          border_attr("Border.solid()", Border.solid()),
          border_attr(
            "Border.shadow(offset: {3, 3}, blur: 8, color: :black)",
            Border.shadow(offset: {3, 3}, blur: 8, color: :black)
          ),
          border_attr(
            "Border.shadow(offset: {0, 0}, blur: 14, size: 1, color: :blue)",
            Border.shadow(offset: {0, 0}, blur: 14, size: 1, color: :blue)
          )
        ]
      ),
      border_card_spec(
        "Inset + glow + dotted",
        "Contrasting inner and outer effects",
        [
          border_attr("Border.rounded(10)", Border.rounded(10)),
          border_attr("Border.width(2)", Border.width(2)),
          border_attr("Border.color(:cyan)", Border.color(:cyan)),
          border_attr("Border.dotted()", Border.dotted()),
          border_attr("Border.glow(:magenta, 3)", Border.glow(:magenta, 3)),
          border_attr(
            "Border.inner_shadow(offset: {2, 2}, blur: 8, color: :purple)",
            Border.inner_shadow(offset: {2, 2}, blur: 8, color: :purple)
          )
        ]
      )
    ]
  end

  defp border_style_variants() do
    [
      %{name: "solid", style_attr: Border.solid(), style_label: "Border.solid()", color: :teal},
      %{
        name: "dashed",
        style_attr: Border.dashed(),
        style_label: "Border.dashed()",
        color: :orange
      },
      %{
        name: "dotted",
        style_attr: Border.dotted(),
        style_label: "Border.dotted()",
        color: :magenta
      }
    ]
  end

  defp single_side_widths() do
    [
      {"Top only", 3, 0, 0, 0},
      {"Right only", 0, 3, 0, 0},
      {"Bottom only", 0, 0, 3, 0},
      {"Left only", 0, 0, 0, 3}
    ]
  end

  defp border_attr(label, attr), do: {label, attr}

  defp border_card_spec(title, subtitle, border_attrs, card_attrs \\ nil) do
    %{
      title: title,
      subtitle: subtitle,
      border_attrs: border_attrs,
      card_attrs: card_attrs || [Background.color(@event_bg)]
    }
  end

  defp border_showcase_grid(cards) do
    wrapped_row([width(fill()), spacing_xy(12, 12)], Enum.map(cards, &border_showcase_card/1))
  end

  defp border_showcase_card(card) do
    border_attrs = card.border_attrs
    border_attr_values = Enum.map(border_attrs, fn {_label, attr} -> attr end)
    border_attr_labels = Enum.map(border_attrs, fn {label, _attr} -> label end)

    el(
      [
        width(px(300)),
        padding(14)
      ] ++ card.card_attrs ++ border_attr_values,
      column([spacing(4)], [
        el([Font.size(13), Font.color(:white)], text(card.title)),
        el([Font.size(11), Font.color(@dim_text)], text(card.subtitle)),
        el([Font.size(10), Font.color({:color_rgb, {200, 200, 220}})], text("Border attrs:")),
        column(
          [spacing(2)],
          Enum.map(border_attr_labels, fn label ->
            el([Font.size(10), Font.color(@dim_text)], text(label))
          end)
        )
      ])
    )
  end

  defp centered_wrapped_cards(cards, max_width) do
    el(
      [center_x(), width(maximum(max_width, fill()))],
      wrapped_row([width(fill()), spacing_xy(12, 12)], cards)
    )
  end

  defp asset_behavior_card(%{
         title: title,
         source_label: source_label,
         status: {status_label, status_tone},
         preview: preview_spec
       }) do
    el(
      [
        width(px(300)),
        padding(10),
        spacing(8),
        Background.color({:color_rgb, {50, 50, 74}}),
        Border.rounded(10)
      ],
      column([spacing(8)], [
        row([width(fill()), spacing(8)], [
          el([width(fill()), Font.size(12), Font.color(:white)], text(title)),
          source_status_chip(status_label, status_tone)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(source_label)),
        el(
          [
            width(fill()),
            height(px(170)),
            padding(8),
            Background.color({:color_rgb, {34, 34, 50}}),
            Border.rounded(8)
          ],
          asset_behavior_preview(preview_spec)
        )
      ])
    )
  end

  defp font_asset_card(%{
         title: title,
         source_label: source_label,
         status: {status_label, status_tone},
         note: note,
         attrs: font_attrs
       }) do
    sample = "quick brown fox jumps over a lazy dog"

    el(
      [
        width(px(300)),
        padding(10),
        spacing(8),
        Background.color({:color_rgb, {50, 50, 74}}),
        Border.rounded(10)
      ],
      column([spacing(8)], [
        row([width(fill()), spacing(8)], [
          el([width(fill()), Font.size(12), Font.color(:white)], text(title)),
          source_status_chip(status_label, status_tone)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(source_label)),
        el([Font.size(10), Font.color({:color_rgb, {196, 202, 222}})], text(note)),
        el(
          [
            width(fill()),
            padding(10),
            Background.color({:color_rgb, {34, 34, 50}}),
            Border.rounded(8)
          ],
          column([spacing(6)], [
            el([Font.size(22), Font.color(:white)] ++ font_attrs, text("Asset Fonts 123")),
            el(
              [Font.size(12), Font.color({:color_rgb, {214, 220, 236}})] ++ font_attrs,
              text(sample)
            )
          ])
        )
      ])
    )
  end

  defp asset_behavior_preview({:image, source, fit, mode_label}) do
    el(
      [
        width(fill()),
        height(fill()),
        Border.width(1),
        Border.color({:color_rgba, {214, 220, 236, 220}}),
        Border.rounded(8),
        clip(),
        in_front(asset_preview_mode_badge(mode_label))
      ],
      image(source, [width(fill()), height(fill()), image_fit(fit)])
    )
  end

  defp asset_behavior_preview({:background, bg_attr, mode_label}) do
    el(
      [
        width(fill()),
        height(fill()),
        bg_attr,
        Border.width(1),
        Border.color({:color_rgba, {214, 220, 236, 220}}),
        Border.rounded(8),
        clip(),
        in_front(asset_preview_mode_badge(mode_label))
      ],
      none()
    )
  end

  defp asset_preview_mode_badge(label) do
    el(
      [
        align_right(),
        align_bottom(),
        move_x(-6),
        move_y(-6),
        padding_each(2, 6, 2, 6),
        Background.color({:color_rgba, {0, 0, 0, 165}}),
        Border.rounded(4),
        Font.size(9),
        Font.color(:white)
      ],
      text(label)
    )
  end

  defp source_status_chip(label, tone) do
    bg_color =
      case tone do
        :source -> {:color_rgb, {58, 98, 158}}
        :runtime -> {:color_rgb, {48, 120, 102}}
        :blocked -> {:color_rgb, {150, 77, 83}}
        :background -> {:color_rgb, {92, 80, 164}}
        :helper -> {:color_rgb, {88, 92, 124}}
        :font_builtin -> {:color_rgb, {84, 106, 94}}
        :font -> {:color_rgb, {132, 86, 54}}
        :synthetic -> {:color_rgb, {118, 74, 120}}
      end

    el(
      [
        padding_each(2, 8, 2, 8),
        Background.color(bg_color),
        Border.rounded(999),
        Font.size(10),
        Font.color(:white)
      ],
      text(label)
    )
  end

  defp fit_demo_card(api_label, frame_label, {frame_w, frame_h}, fit, variant, source) do
    stage_padding = 10
    stage_w = frame_w + stage_padding * 2
    stage_h = frame_h + stage_padding * 2
    card_w = max(stage_w + 20, 220)

    el(
      [
        width(px(card_w)),
        padding(10),
        spacing(8),
        Background.color({:color_rgb, {45, 45, 68}}),
        Border.rounded(10)
      ],
      column([spacing(8)], [
        row([width(fill()), spacing(8)], [
          el([Font.size(11), Font.color(:white)], text(api_label)),
          fit_chip(fit)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(frame_label)),
        el(
          [Font.size(10), Font.color({:color_rgb, {184, 188, 210}})],
          text("#{frame_w}x#{frame_h}")
        ),
        el(
          [
            center_x(),
            width(px(stage_w)),
            height(px(stage_h)),
            Background.color({:color_rgb, {31, 31, 45}}),
            Border.rounded(8)
          ],
          fit_demo_preview(variant, source, fit, {frame_w, frame_h})
        )
      ])
    )
  end

  defp fit_demo_preview(:element, source, fit, {frame_w, frame_h}) do
    el(
      [
        center_x(),
        center_y(),
        width(px(frame_w)),
        height(px(frame_h)),
        Background.color({:color_rgb, {24, 24, 36}}),
        Border.width(1),
        Border.color({:color_rgba, {214, 220, 236, 220}}),
        Border.rounded(8),
        clip()
      ],
      image(source, [width(fill()), height(fill()), image_fit(fit)])
    )
  end

  defp fit_demo_preview(:background, source, fit, {frame_w, frame_h}) do
    el(
      [
        center_x(),
        center_y(),
        width(px(frame_w)),
        height(px(frame_h)),
        Background.image(source, fit: fit),
        Border.width(1),
        Border.color({:color_rgba, {214, 220, 236, 220}}),
        Border.rounded(8),
        clip()
      ],
      el(
        [
          center_x(),
          center_y(),
          padding(5),
          Background.color({:color_rgba, {0, 0, 0, 160}}),
          Border.rounded(5),
          Font.size(10),
          Font.color(:white)
        ],
        text("bg")
      )
    )
  end

  defp fit_chip(:contain) do
    el(
      [
        padding(4),
        Background.color({:color_rgb, {52, 110, 124}}),
        Border.rounded(6),
        Font.size(10),
        Font.color(:white)
      ],
      text("contain")
    )
  end

  defp fit_chip(:cover) do
    el(
      [
        padding(4),
        Background.color({:color_rgb, {142, 84, 52}}),
        Border.rounded(6),
        Font.size(10),
        Font.color(:white)
      ],
      text("cover")
    )
  end

  defp fit_legend() do
    column([spacing(4)], [
      el([Font.size(11), Font.color({:color_rgb, {200, 210, 222}})], text("Fit legend")),
      el(
        [Font.size(10), Font.color(@dim_text)],
        text("contain: full image visible, may letterbox")
      ),
      el([Font.size(10), Font.color(@dim_text)], text("cover: frame fully filled, may crop"))
    ])
  end

  defp section_title(label) do
    el([Font.size(16), Font.color(@light_text)], text(label))
  end

  defp menu_item(label, page, current_page) do
    active = page == current_page

    bg = if active, do: @blue, else: {:color_rgb, {45, 45, 65}}
    text_color = if active, do: @light_text, else: @dim_text

    hover_attrs =
      if active do
        []
      else
        [
          mouse_over([
            Background.color({:color_rgb, {70, 70, 100}}),
            Font.color(@light_text)
          ])
        ]
      end

    el(
      [
        width(fill()),
        padding(10),
        Background.color(bg),
        Border.rounded(10),
        on_click({self(), {:demo_nav, page}})
      ] ++ hover_attrs,
      el([Font.size(12), Font.color(text_color)], text(label))
    )
  end

  defp drain_events(acc \\ []) do
    receive do
      {:emerge_skia_event, _} = message -> drain_events([message | acc])
      {:demo_event, _, _} = message -> drain_events([message | acc])
      {:demo_nav, _} = message -> drain_events([message | acc])
      {:feature_click, _} = message -> drain_events([message | acc])
      {:clock_tick, _} = message -> drain_events([message | acc])
      :scramble_unstable = message -> drain_events([message | acc])
      {:unstable_row_click, _} = message -> drain_events([message | acc])
      {:unstable_child_click, _, _} = message -> drain_events([message | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  defp render_update(renderer, state, tree) do
    {next_state, _assigned} = EmergeSkia.patch_tree(renderer, state, tree)
    next_state
  end

  defp handle_event(
         renderer,
         state,
         mouse_pos,
         event_log,
         size,
         scale,
         current_page,
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
          last_move_label,
          unstable_items
        )

      {:clock_tick, _} = message ->
        process_event_batch(
          [message | drain_events()],
          renderer,
          state,
          mouse_pos,
          event_log,
          size,
          scale,
          current_page,
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
         last_move_label,
         unstable_items
       ) do
    log_input = Process.get(:log_input, false)
    log_render = Process.get(:log_render, false)

    if log_input do
      batch_last_cursor =
        Enum.reduce(events, nil, fn message, acc ->
          case message do
            {:emerge_skia_event, {:cursor_pos, {x, y}}} -> {x, y}
            _ -> acc
          end
        end)

      IO.puts("demo batch size=#{length(events)} last_cursor=#{inspect(batch_last_cursor)}")
    end

    {next_state, needs_render} =
      Enum.reduce(
        events,
        {
          {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items},
          false
        },
        fn message, {acc, dirty} ->
          {next_acc, changed} = process_event(message, state, acc)
          {next_acc, dirty or changed}
        end
      )

    {new_mouse_pos, new_log, new_size, new_scale, new_page, new_move, new_unstable} =
      next_state

    if needs_render do
      render_seq = Process.get(:render_seq, 0) + 1
      Process.put(:render_seq, render_seq)

      tree =
        build_tree(
          new_size,
          new_mouse_pos,
          new_log,
          new_page,
          new_move,
          new_unstable
        )

      if log_render do
        IO.puts("demo render_seq=#{render_seq} mouse_pos=#{inspect(new_mouse_pos)}")
      end

      next_state = render_update(renderer, state, tree)

      {:ok, next_state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_move,
       new_unstable}
    else
      {:ok, state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_move, new_unstable}
    end
  end

  defp process_event(
         {:emerge_skia_event, event},
         state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    if Process.get(:log_input, false) do
      case event do
        {:cursor_pos, {x, y}} ->
          IO.puts("demo event cursor_pos=#{Float.round(x, 2)},#{Float.round(y, 2)}")

        _ ->
          :ok
      end
    end

    {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items} =
      case event do
        {id_bin, event_type} when is_binary(id_bin) and is_atom(event_type) ->
          case Emerge.lookup_event(state, id_bin, event_type) do
            {:ok, {pid, msg}} when pid == self() ->
              {next_state, _changed} =
                process_event(
                  msg,
                  state,
                  {mouse_pos, event_log, size, scale, current_page, last_move_label,
                   unstable_items}
                )

              next_state

            _ ->
              Emerge.dispatch_event(state, id_bin, event_type)

              {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
          end

        {id_bin, event_type, payload} when is_binary(id_bin) and is_atom(event_type) ->
          case Emerge.lookup_event(state, id_bin, event_type) do
            {:ok, {pid, msg}} when pid == self() ->
              msg_with_payload =
                if is_tuple(msg),
                  do: Tuple.insert_at(msg, tuple_size(msg), payload),
                  else: {msg, payload}

              {next_state, _changed} =
                process_event(
                  msg_with_payload,
                  state,
                  {mouse_pos, event_log, size, scale, current_page, last_move_label,
                   unstable_items}
                )

              next_state

            _ ->
              Emerge.dispatch_event(state, id_bin, event_type, payload)

              {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
          end

        _ ->
          {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
      end

    preedit_changed =
      case event do
        {:text_preedit, {text, cursor}} when is_binary(text) ->
          previous_text = Process.get(:demo_input_preedit, nil)
          previous_cursor = Process.get(:demo_input_preedit_cursor, nil)
          Process.put(:demo_input_preedit, text)
          Process.put(:demo_input_preedit_cursor, cursor)
          previous_text != text or previous_cursor != cursor

        :text_preedit_clear ->
          previous_text = Process.get(:demo_input_preedit, nil)
          previous_cursor = Process.get(:demo_input_preedit_cursor, nil)
          Process.put(:demo_input_preedit, nil)
          Process.put(:demo_input_preedit_cursor, nil)
          previous_text != nil or previous_cursor != nil

        _ ->
          false
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
        new_scale != scale or preedit_changed

    {{new_mouse_pos, new_log, new_size, new_scale, current_page, last_move_label, unstable_items},
     changed}
  end

  defp process_event(
         {:clock_tick, time_string},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    Process.put(:clock_time, time_string)

    {{mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}, true}
  end

  defp process_event(
         {:feature_click, title},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    new_log = Enum.take(["UI Click: #{title}" | event_log], 20)

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items},
     new_log != event_log}
  end

  defp process_event(
         {:demo_nav, page},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    new_log = Enum.take(["Navigate: #{format_page(page)}" | event_log], 20)
    new_page = page
    changed = new_log != event_log or new_page != current_page

    {{mouse_pos, new_log, size, scale, new_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :hover_manual, hover_event},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       )
       when hover_event in [:mouse_enter, :mouse_leave] do
    active = hover_event == :mouse_enter
    previous = Process.get(:hover_manual_active, false)
    Process.put(:hover_manual_active, active)

    entry = if active, do: "Manual hover: enter", else: "Manual hover: leave"
    new_log = Enum.take([entry | event_log], 20)
    changed = previous != active or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :inupt_changed, value},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       )
       when is_binary(value) do
    previous = Process.get(:demo_input_value, "")
    Process.put(:demo_input_value, value)
    Process.put(:demo_input_preedit, nil)
    Process.put(:demo_input_preedit_cursor, nil)

    changed_value = value != previous

    new_log =
      if changed_value do
        preview =
          if String.length(value) > 42, do: String.slice(value, 0, 39) <> "...", else: value

        Enum.take(["Inupt change: #{preview}" | event_log], 20)
      else
        event_log
      end

    changed = changed_value or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :inupt_focus},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_input_focused, false)
    count = Process.get(:demo_input_focus_count, 0) + 1

    Process.put(:demo_input_focused, true)
    Process.put(:demo_input_focus_count, count)

    new_log = Enum.take(["Inupt focus (#{count})" | event_log], 20)
    changed = !previous or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :inupt_blur},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_input_focused, false)
    count = Process.get(:demo_input_blur_count, 0) + 1

    Process.put(:demo_input_focused, false)
    Process.put(:demo_input_blur_count, count)

    new_log = Enum.take(["Inupt blur (#{count})" | event_log], 20)
    changed = previous or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, label, event},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
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

    {{mouse_pos, new_log, size, scale, current_page, new_move_label, unstable_items}, changed}
  end

  defp process_event(
         :scramble_unstable,
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    {new_items, child_assignments} = scramble_unstable_items(unstable_items)
    new_log = Enum.take(["Scramble: unstable list" | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log or child_assignments > 0

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, new_items}, changed}
  end

  defp process_event(
         {:unstable_row_click, label},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    {new_items, next_count} =
      update_unstable_item(unstable_items, label, fn item ->
        updated = %{item | count: item.count + 1}
        {updated, updated.count}
      end)

    new_log = Enum.take(["Unstable row: #{label} (#{next_count})" | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, new_items}, changed}
  end

  defp process_event(
         {:unstable_child_click, parent_label, child_label},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    {new_items, child_count} = update_unstable_child(unstable_items, child_label)

    entry = "Unstable child: #{child_label} (#{child_count}) in #{parent_label}"
    new_log = Enum.take([entry | event_log], 20)
    changed = new_items != unstable_items or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, new_items}, changed}
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
            last_move_label,
            unstable_items
          )

        {:ok, next_state, new_mouse_pos, new_log, new_size, new_scale, new_page, new_move_label,
         new_unstable_items} ->
          run_loop(
            renderer,
            next_state,
            new_mouse_pos,
            new_log,
            new_size,
            new_scale,
            new_page,
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

Process.put(:render_seq, 0)

initial_size = {width * 1.0, height * 1.0}
initial_scale = 1.0
initial_mouse = {elem(initial_size, 0) / 2, elem(initial_size, 1) / 2}

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
  Demo.build_tree(initial_size, initial_mouse, [], :overview, nil, initial_unstable_items)

{state, _assigned} = EmergeSkia.upload_tree(renderer, initial_tree)

Demo.run_loop(
  renderer,
  state,
  initial_mouse,
  [],
  initial_size,
  initial_scale,
  :overview,
  nil,
  initial_unstable_items
)

IO.puts("Window closed. Demo complete!")
