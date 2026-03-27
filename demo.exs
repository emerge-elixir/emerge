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
    aliases: [
      b: :backend,
      c: :card,
      w: :width,
      h: :height,
      i: :input_log,
      r: :render_log
    ]
  )

backend = Keyword.get(cli_opts, :backend, "wayland")
width = Keyword.get(cli_opts, :width, 3840)
height = Keyword.get(cli_opts, :height, 2160)
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

startup_detail =
  if card,
    do: " backend=#{backend} card=#{card}",
    else: " backend=#{backend}"

IO.puts("Starting EmergeSkia demo..." <> startup_detail)

blocked_root = Path.join(System.tmp_dir!(), "emerge_skia_demo_blocked")

demo_priv_dir =
  case :code.priv_dir(:emerge) do
    path when is_list(path) -> List.to_string(path)
    _ -> raise "failed to resolve :emerge priv dir"
  end

demo_image_root = Path.join(demo_priv_dir, "demo_images")
demo_font_root = Path.join(demo_priv_dir, "demo_fonts")

required_demo_assets = [
  "static.jpg",
  "runtime.jpg",
  "fallback.jpg",
  "tile_bird_small.jpg",
  "template_cloud.svg",
  "weather_sun.svg",
  "weather_cloud.svg",
  "weather_rain.svg"
]

required_demo_fonts = ["Lobster-Regular.ttf"]

missing_demo_assets =
  Enum.filter(required_demo_assets, fn file_name ->
    not File.regular?(Path.join(demo_image_root, file_name))
  end)

if missing_demo_assets != [] do
  raise "missing demo assets in #{demo_image_root}: #{Enum.join(missing_demo_assets, ", ")}"
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
      extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]
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
Process.put(:animation_shelf_open, false)
Process.put(:demo_input_value, "quick brown fox")
Process.put(:demo_input_preedit, nil)
Process.put(:demo_input_preedit_cursor, nil)
Process.put(:demo_input_focused, false)
Process.put(:demo_input_focus_count, 0)
Process.put(:demo_input_blur_count, 0)
Process.put(:demo_button_focused, false)
Process.put(:demo_button_press_count, 0)
Process.put(:demo_button_focus_count, 0)
Process.put(:demo_button_blur_count, 0)
Process.put(:demo_key_listener_focused, false)
Process.put(:demo_key_listener_focus_count, 0)
Process.put(:demo_key_listener_blur_count, 0)
Process.put(:demo_key_listener_key_down_count, 0)
Process.put(:demo_key_listener_key_up_count, 0)
Process.put(:demo_key_listener_key_press_count, 0)
Process.put(:demo_key_listener_enter_count, 0)
Process.put(:demo_key_listener_ctrl_digit_count, 0)
Process.put(:demo_key_listener_arrow_left_count, 0)
Process.put(:demo_key_listener_escape_count, 0)
Process.put(:demo_key_listener_space_press_count, 0)
Process.put(:demo_key_listener_last_action, "Nothing yet")
Process.put(:demo_swipe_last_direction, "None")
Process.put(:demo_swipe_up_count, 0)
Process.put(:demo_swipe_down_count, 0)
Process.put(:demo_swipe_left_count, 0)
Process.put(:demo_swipe_right_count, 0)
Process.put(:demo_soft_shift, false)
Process.put(:demo_soft_popup, nil)
Process.put(:demo_soft_hold_count, 0)
Process.put(:demo_soft_last_action, "Tap the text input, then use the soft keys below.")

clock_loop = fn loop ->
  send(demo_pid, {:clock_tick, clock_now.()})
  Process.sleep(1000)
  loop.(loop)
end

spawn(fn -> clock_loop.(clock_loop) end)

defmodule Demo do
  use Emerge.UI

  @dark_bg color_rgba(26, 26, 46, 1.0)
  @blue color_rgba(67, 97, 238, 1.0)
  @purple color_rgba(114, 9, 183, 1.0)
  @pink color_rgba(247, 37, 133, 1.0)
  @light_text color_rgba(255, 255, 255, 1.0)
  @dim_text color_rgba(170, 170, 170, 1.0)
  @event_bg color_rgba(45, 45, 68, 1.0)

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

  def format_event({id_bin, :key_down, route}) when is_binary(id_bin) and is_binary(route) do
    "Key down: #{format_key_route(route)} on #{inspect(:erlang.binary_to_term(id_bin))}"
  end

  def format_event({id_bin, :key_up, route}) when is_binary(id_bin) and is_binary(route) do
    "Key up: #{format_key_route(route)} on #{inspect(:erlang.binary_to_term(id_bin))}"
  end

  def format_event({id_bin, :key_press, route}) when is_binary(id_bin) and is_binary(route) do
    "Key press: #{format_key_route(route)} on #{inspect(:erlang.binary_to_term(id_bin))}"
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

  defp format_key_route(route) when is_binary(route) do
    case String.split(route, ":", parts: 4) do
      [_action, key_name, match, mods_mask] ->
        mods =
          case Integer.parse(mods_mask) do
            {mask, ""} -> decode_key_route_modifiers(mask)
            _ -> []
          end

        combo = format_key_combo(key_name, mods)

        if match == "exact", do: combo, else: "#{combo} (#{match})"

      _ ->
        route
    end
  end

  defp decode_key_route_modifiers(mask) when is_integer(mask) do
    [
      Bitwise.band(mask, 0x01) != 0 && :shift,
      Bitwise.band(mask, 0x02) != 0 && :ctrl,
      Bitwise.band(mask, 0x04) != 0 && :alt,
      Bitwise.band(mask, 0x08) != 0 && :meta
    ]
    |> Enum.reject(&(&1 == false))
  end

  defp format_key_combo(key_name, mods) when is_binary(key_name) and is_list(mods) do
    (Enum.map(mods, &format_key_modifier/1) ++ [format_key_name(key_name)])
    |> Enum.join("+")
  end

  defp format_key_modifier(:shift), do: "Shift"
  defp format_key_modifier(:ctrl), do: "Ctrl"
  defp format_key_modifier(:alt), do: "Alt"
  defp format_key_modifier(:meta), do: "Meta"

  defp format_key_name("digit_" <> digit), do: digit

  defp format_key_name(key_name) when is_binary(key_name) do
    key_name
    |> String.split("_")
    |> Enum.map_join(" ", &String.capitalize/1)
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
      {"Animation", :animation},
      {"Events", :events},
      {"Hover", :hover},
      {"Unstable List", :unstable_list},
      {"Nearby", :nearby},
      {"Text", :text},
      {"Input", :input},
      {"Borders", :borders},
      {"Assets", :assets}
    ]

    column(
      [
        width(px(220)),
        height(fill()),
        scrollbar_y(),
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
        el([Font.size(11), Font.color(@dim_text)], text("Click or focus + Enter to switch"))
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
        Background.color(color_rgb(35, 35, 55)),
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
          Background.color(color_rgb(35, 35, 55)),
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
    page =
      case current_page do
        :overview -> page_overview()
        :layout -> page_layout()
        :scroll -> page_scroll()
        :alignment -> page_alignment()
        :transforms -> page_transforms()
        :animation -> page_animation()
        :events -> page_events(last_move_label)
        :hover -> page_hover()
        :unstable_list -> page_unstable_list(unstable_items)
        :nearby -> page_nearby()
        :text -> page_text()
        :input -> page_input()
        :borders -> page_borders()
        :assets -> page_assets()
        _ -> page_overview()
      end

    with_page_key(page, current_page)
  end

  defp with_page_key(%Emerge.Engine.Element{} = page, current_page) do
    %{page | id: {:page, current_page}}
  end

  defp page_overview() do
    column([width(fill()), spacing(16)], [
      el([Font.size(22), Font.color(:white)], text("Overview")),
      el(
        [Font.size(13), Font.color(@dim_text)],
        text("Explore layout, scrolling, alignment, and transform demos from the menu.")
      ),
      row([width(fill()), spacing(12)], [
        feature_card("Rows", "Horizontal layouts", color_rgb(60, 60, 120)),
        feature_card("Columns", "Vertical layouts", color_rgb(60, 90, 60)),
        feature_card("Nesting", "Compose layouts", color_rgb(90, 60, 90))
      ]),
      el(
        [
          width(fill()),
          padding(14),
          Background.color(color_rgb(60, 50, 80)),
          Border.rounded_each(18, 6, 22, 10)
        ],
        column([spacing(6)], [
          el([Font.size(16), Font.color(:white)], text("Per-corner radius")),
          el(
            [Font.size(12), Font.color(color_rgb(200, 200, 220))],
            text("Each corner can be different")
          )
        ])
      ),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(12),
            Background.color(color_rgb(50, 70, 90)),
            Border.rounded(10),
            Transform.rotate(-6),
            Transform.alpha(0.85)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Rotate + alpha")),
            el([Font.size(11), Font.color(color_rgb(200, 220, 230))], text("-6deg, 85%"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(12),
            Background.color(color_rgb(70, 60, 90)),
            Border.rounded(10),
            Transform.scale(1.06),
            Transform.move_y(-14)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Scale + move")),
            el([Font.size(11), Font.color(color_rgb(220, 210, 235))], text("1.06x, -4px"))
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
    template_cloud_source = "demo_images/template_cloud.svg"
    sun_source = "demo_images/weather_sun.svg"
    cloud_source = "demo_images/weather_cloud.svg"
    rain_source = "demo_images/weather_rain.svg"
    font_source = Process.get(:demo_font_source, "demo_fonts/Lobster-Regular.ttf")
    weather_forecast = weather_forecast_data()

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

    svg_fit_cards =
      Enum.flat_map(fit_frames, fn {label, {frame_w, frame_h}} ->
        Enum.map([:contain, :cover], fn fit ->
          fit_demo_card(
            "svg/2",
            label,
            {frame_w, frame_h},
            fit,
            :svg,
            cloud_source
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
          title: "Static SVG source",
          source_label: ~s(source: "demo_images/weather_sun.svg"),
          status: {"Source root", :source},
          preview: {:svg, sun_source, :contain, "svg :contain"}
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
          "Assets resolve from otp_app priv or runtime paths, then render through image/2, Background helpers, startup-loaded font assets, and vector SVG icons."
        )
      ),
      section_title("SVG Weather"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "A hardcoded seven-day forecast using local SVG icons. Temperatures lead with Celsius and keep Fahrenheit as the quieter secondary scale."
        )
      ),
      weather_widget_card(weather_forecast),
      svg_weather_scale_showcase([
        {"Sun", "Bright icon reused from forecast cells to oversized hero scale.", sun_source},
        {"Cloud", "Soft neutral linework rendered across compact and roomy card slots.",
         cloud_source},
        {"Rain", "Same source file reused for small forecast markers and larger detail art.",
         rain_source}
      ]),
      svg_tint_showcase(template_cloud_source, "demo_images/tile_quad.svg"),
      el(
        [Font.size(12), Font.color(color_rgb(205, 214, 229))],
        text("svg/2 fit behavior")
      ),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text(
          "The same SVG source uses the regular contain/cover rules inside wide, tall, and square frames."
        )
      ),
      centered_wrapped_cards(svg_fit_cards, 960),
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
        text(
          "Tile source: demo_images/tile_bird_small.jpg (160x120). SVG backgrounds are also supported through the same API."
        )
      ),
      el(
        [Font.size(12), Font.color(color_rgb(205, 214, 229))],
        text("Background.image/2 fit behavior")
      ),
      centered_wrapped_cards(background_fit_cards, 960),
      el(
        [
          width(fill()),
          padding(10),
          spacing(6),
          Background.color(color_rgb(48, 48, 72)),
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

  defp page_input() do
    value = Process.get(:demo_input_value, "quick brown fox")
    preedit = Process.get(:demo_input_preedit, nil)
    preedit_cursor = Process.get(:demo_input_preedit_cursor, nil)
    focused = Process.get(:demo_input_focused, false)
    focus_count = Process.get(:demo_input_focus_count, 0)
    blur_count = Process.get(:demo_input_blur_count, 0)
    button_focused = Process.get(:demo_button_focused, false)
    button_press_count = Process.get(:demo_button_press_count, 0)
    button_focus_count = Process.get(:demo_button_focus_count, 0)
    button_blur_count = Process.get(:demo_button_blur_count, 0)
    key_listener_focused = Process.get(:demo_key_listener_focused, false)
    key_listener_focus_count = Process.get(:demo_key_listener_focus_count, 0)
    key_listener_blur_count = Process.get(:demo_key_listener_blur_count, 0)
    key_listener_key_down_count = Process.get(:demo_key_listener_key_down_count, 0)
    key_listener_key_up_count = Process.get(:demo_key_listener_key_up_count, 0)
    key_listener_key_press_count = Process.get(:demo_key_listener_key_press_count, 0)
    key_listener_enter_count = Process.get(:demo_key_listener_enter_count, 0)
    key_listener_ctrl_digit_count = Process.get(:demo_key_listener_ctrl_digit_count, 0)
    key_listener_arrow_left_count = Process.get(:demo_key_listener_arrow_left_count, 0)
    key_listener_escape_count = Process.get(:demo_key_listener_escape_count, 0)
    key_listener_space_press_count = Process.get(:demo_key_listener_space_press_count, 0)
    key_listener_last_action = Process.get(:demo_key_listener_last_action, "Nothing yet")

    {status_label, status_bg, status_text, input_border_color, input_border_width} =
      if focused do
        {
          "Focused",
          color_rgb(72, 96, 70),
          color_rgb(227, 244, 223),
          color_rgb(228, 183, 104),
          1
        }
      else
        {
          "Blurred",
          color_rgb(72, 74, 102),
          color_rgb(220, 224, 240),
          color_rgb(120, 130, 175),
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

    {button_state_label, button_state_bg, button_state_text} =
      if button_focused do
        {
          "Focused",
          color_rgb(70, 96, 82),
          color_rgb(224, 244, 236)
        }
      else
        {
          "Blurred",
          color_rgb(72, 74, 102),
          color_rgb(220, 224, 240)
        }
      end

    {key_listener_state_label, key_listener_state_bg, key_listener_state_text} =
      if key_listener_focused do
        {
          "Focused",
          color_rgb(70, 96, 82),
          color_rgb(224, 244, 236)
        }
      else
        {
          "Blurred",
          color_rgb(72, 74, 102),
          color_rgb(220, 224, 240)
        }
      end

    column([width(fill()), spacing(16)], [
      section_title("Input"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Text input: tab/shift+tab focus cycle, click/drag select, shift+arrows, ctrl/meta+a/c/x/v, middle-click paste, backspace/delete."
        )
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(48, 48, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          Emerge.UI.Input.text(
            [
              width(fill()),
              padding_xy(10, 8),
              Font.size(16),
              Font.color(:white),
              Background.color(color_rgb(62, 62, 94)),
              Border.rounded(8),
              Border.width(input_border_width),
              Border.color(input_border_color),
              Event.on_change({self(), {:demo_event, :input_changed}}),
              Event.on_focus({self(), {:demo_event, :input_focus}}),
              Event.on_blur({self(), {:demo_event, :input_blur}})
            ],
            value
          ),
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
                Background.color(color_rgb(64, 74, 106)),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(color_rgb(205, 216, 246))],
                text("focus: #{focus_count}")
              )
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color(color_rgb(78, 68, 100)),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(color_rgb(228, 212, 246))],
                text("blur: #{blur_count}")
              )
            )
          ]),
          el(
            [Font.size(12), Font.color(color_rgb(225, 228, 244))],
            text("Value: #{value_label}")
          ),
          el([Font.size(11), Font.color(@dim_text)], text("Length: #{String.length(value)}")),
          el([Font.size(11), Font.color(@dim_text)], text("Preedit: #{preedit_label}")),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text("Preedit cursor: #{inspect(preedit_cursor)}")
          )
        ])
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(46, 50, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          el(
            [Font.size(12), Font.color(color_rgb(225, 230, 244))],
            text("Declarative interaction styles")
          ),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text(
              "No focus/down handlers required: styles come from mouse_over, focused, and mouse_down."
            )
          ),
          Emerge.UI.Input.text(
            [
              width(fill()),
              padding_xy(10, 8),
              Font.size(16),
              Font.color(color_rgb(228, 232, 246)),
              Background.color(color_rgb(58, 62, 90)),
              Border.rounded(8),
              Border.width(1),
              Border.color(color_rgb(110, 120, 162)),
              Interactive.mouse_over([
                Background.color(color_rgb(64, 70, 100)),
                Border.color(color_rgb(132, 143, 189))
              ]),
              Interactive.focused([
                Background.color(color_rgb(70, 78, 112)),
                Border.color(color_rgb(164, 188, 236))
              ]),
              Interactive.mouse_down([
                Background.color(color_rgb(63, 70, 100)),
                Border.color(color_rgb(224, 186, 124)),
                Transform.move_y(1)
              ])
            ],
            "Style showcase input"
          ),
          el(
            [Font.size(10), Font.color(@dim_text)],
            text("Merge order: mouse_over -> focused -> mouse_down (later styles win conflicts).")
          )
        ])
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(46, 50, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          el(
            [Font.size(12), Font.color(color_rgb(225, 230, 244))],
            text("Input.button + on_press")
          ),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text("Press fires on click, and also on Enter when this button is focused.")
          ),
          Emerge.UI.Input.button(
            [
              width(fill()),
              padding_xy(10, 8),
              Font.size(14),
              Font.color(color_rgb(230, 234, 246)),
              Background.color(color_rgb(58, 62, 90)),
              Border.rounded(8),
              Border.width(1),
              Border.color(color_rgb(110, 120, 162)),
              Event.on_press({self(), {:demo_event, :button_press}}),
              Event.on_focus({self(), {:demo_event, :button_focus}}),
              Event.on_blur({self(), {:demo_event, :button_blur}}),
              Interactive.mouse_over([
                Background.color(color_rgb(64, 70, 100)),
                Border.color(color_rgb(132, 143, 189))
              ]),
              Interactive.focused([
                Border.color(color_rgb(166, 186, 236)),
                Border.glow(color_rgba(132, 158, 232, 100 / 255), 2)
              ]),
              Interactive.mouse_down([
                Background.color(color_rgb(56, 60, 88)),
                Border.color(color_rgb(176, 190, 228)),
                Border.inner_shadow(
                  offset: {0, 1},
                  blur: 6,
                  size: 1,
                  color: color_rgba(0, 0, 0, 120 / 255)
                ),
                Transform.move_y(1)
              ])
            ],
            text("Run action")
          ),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            el(
              [
                padding_xy(10, 5),
                Background.color(button_state_bg),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(button_state_text)],
                text("State: #{button_state_label}")
              )
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color(color_rgb(66, 74, 108)),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(color_rgb(208, 218, 246))],
                text("press: #{button_press_count}")
              )
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color(color_rgb(64, 82, 96)),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(color_rgb(210, 238, 236))],
                text("focus: #{button_focus_count}")
              )
            ),
            el(
              [
                padding_xy(10, 5),
                Background.color(color_rgb(78, 68, 100)),
                Border.rounded(999)
              ],
              el(
                [Font.size(11), Font.color(color_rgb(228, 212, 246))],
                text("blur: #{button_blur_count}")
              )
            )
          ]),
          el(
            [Font.size(10), Font.color(@dim_text)],
            text("Try Tab/Shift+Tab to focus this button, then press Enter to trigger on_press.")
          )
        ])
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(46, 50, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          el(
            [Font.size(12), Font.color(color_rgb(225, 230, 244))],
            text("Focused key listeners")
          ),
          el(
            [Font.size(11), Font.color(@dim_text)],
            text(
              "Click or Tab onto the card, then try on_key_down(:enter), on_key_down([key: :digit_1, mods: [:ctrl], match: :all]), on_key_down(:arrow_left), on_key_up(:escape), and on_key_press(:space)."
            )
          ),
          el(
            [
              width(fill()),
              padding(14),
              spacing(8),
              Background.color(color_rgb(56, 60, 88)),
              Border.rounded(10),
              Border.width(1),
              Border.color(color_rgb(112, 122, 164)),
              Event.on_focus({self(), {:demo_event, :keyboard_listener, :focus}}),
              Event.on_blur({self(), {:demo_event, :keyboard_listener, :blur}}),
              Event.on_key_down(
                :enter,
                {self(), {:demo_event, :keyboard_listener, {:key_down, :enter}}}
              ),
              Event.on_key_down(
                [key: :digit_1, mods: [:ctrl], match: :all],
                {self(), {:demo_event, :keyboard_listener, {:key_down, :ctrl_digit_1}}}
              ),
              Event.on_key_down(
                :arrow_left,
                {self(), {:demo_event, :keyboard_listener, {:key_down, :arrow_left}}}
              ),
              Event.on_key_up(
                :escape,
                {self(), {:demo_event, :keyboard_listener, {:key_up, :escape}}}
              ),
              Event.on_key_press(
                :space,
                {self(), {:demo_event, :keyboard_listener, {:key_press, :space}}}
              ),
              Interactive.mouse_over([
                Background.color(color_rgb(62, 68, 98)),
                Border.color(color_rgb(138, 148, 190))
              ]),
              Interactive.focused([
                Background.color(color_rgb(66, 74, 106)),
                Border.color(color_rgb(176, 196, 244)),
                Border.glow(color_rgba(132, 158, 232, 110 / 255), 2)
              ])
            ],
            column([spacing(8)], [
              el([Font.size(14), Font.color(:white)], text("Keyboard listener pad")),
              el(
                [Font.size(11), Font.color(color_rgb(214, 220, 240))],
                text("Focused-only routing. No on_press handler here - only direct key events.")
              ),
              wrapped_row([width(fill()), spacing_xy(8, 8)], [
                el(
                  [
                    padding_xy(10, 5),
                    Background.color(color_rgb(78, 86, 124)),
                    Border.rounded(999)
                  ],
                  el([Font.size(10), Font.color(color_rgb(232, 238, 252))], text("Enter -> down"))
                ),
                el(
                  [
                    padding_xy(10, 5),
                    Background.color(color_rgb(82, 76, 132)),
                    Border.rounded(999)
                  ],
                  el(
                    [Font.size(10), Font.color(color_rgb(238, 232, 252))],
                    text("Ctrl+1 -> down (all)")
                  )
                ),
                el(
                  [
                    padding_xy(10, 5),
                    Background.color(color_rgb(74, 92, 122)),
                    Border.rounded(999)
                  ],
                  el(
                    [Font.size(10), Font.color(color_rgb(226, 240, 250))],
                    text("Arrow Left -> down")
                  )
                ),
                el(
                  [
                    padding_xy(10, 5),
                    Background.color(color_rgb(98, 76, 112)),
                    Border.rounded(999)
                  ],
                  el([Font.size(10), Font.color(color_rgb(244, 226, 246))], text("Escape -> up"))
                ),
                el(
                  [
                    padding_xy(10, 5),
                    Background.color(color_rgb(108, 86, 120)),
                    Border.rounded(999)
                  ],
                  el(
                    [Font.size(10), Font.color(color_rgb(248, 234, 246))],
                    text("Space -> press")
                  )
                )
              ]),
              el(
                [Font.size(10), Font.color(color_rgb(196, 204, 228))],
                text(
                  "Tip: click once to focus, then hold Ctrl while pressing 1 to hit the modifier matcher. Space completes on key release here, so the press counter updates after the key comes back up."
                )
              )
            ])
          ),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            el(
              [padding_xy(10, 5), Background.color(key_listener_state_bg), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(key_listener_state_text)],
                text("State: #{key_listener_state_label}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(64, 74, 106)), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(color_rgb(205, 216, 246))],
                text("focus: #{key_listener_focus_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(78, 68, 100)), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(color_rgb(228, 212, 246))],
                text("blur: #{key_listener_blur_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(70, 84, 114)), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(color_rgb(220, 232, 248))],
                text("down: #{key_listener_key_down_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(96, 76, 116)), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(color_rgb(244, 228, 246))],
                text("up: #{key_listener_key_up_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(112, 82, 120)), Border.rounded(999)],
              el(
                [Font.size(11), Font.color(color_rgb(248, 236, 246))],
                text("press: #{key_listener_key_press_count}")
              )
            )
          ]),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            el(
              [padding_xy(10, 5), Background.color(color_rgb(76, 88, 134)), Border.rounded(999)],
              el(
                [Font.size(10), Font.color(color_rgb(234, 240, 252))],
                text("Enter: #{key_listener_enter_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(86, 78, 138)), Border.rounded(999)],
              el(
                [Font.size(10), Font.color(color_rgb(240, 234, 252))],
                text("Ctrl+1: #{key_listener_ctrl_digit_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(72, 96, 128)), Border.rounded(999)],
              el(
                [Font.size(10), Font.color(color_rgb(228, 242, 250))],
                text("Arrow Left: #{key_listener_arrow_left_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(102, 76, 120)), Border.rounded(999)],
              el(
                [Font.size(10), Font.color(color_rgb(246, 230, 248))],
                text("Escape up: #{key_listener_escape_count}")
              )
            ),
            el(
              [padding_xy(10, 5), Background.color(color_rgb(114, 84, 126)), Border.rounded(999)],
              el(
                [Font.size(10), Font.color(color_rgb(248, 236, 248))],
                text("Space press: #{key_listener_space_press_count}")
              )
            )
          ]),
          el(
            [Font.size(11), Font.color(color_rgb(230, 234, 246))],
            text("Last action: #{key_listener_last_action}")
          )
        ])
      ),
      soft_keyboard_showcase_card()
    ])
  end

  defp soft_keyboard_showcase_card() do
    shift_active = Process.get(:demo_soft_shift, false)
    popup = Process.get(:demo_soft_popup, nil)
    hold_count = Process.get(:demo_soft_hold_count, 0)

    last_action =
      Process.get(:demo_soft_last_action, "Tap the text input, then use the soft keys below.")

    popup_label =
      case popup do
        nil -> "none"
        :a -> if(shift_active, do: "A popup", else: "a popup")
        :e -> if(shift_active, do: "E popup", else: "e popup")
        other -> inspect(other)
      end

    column([width(fill()), spacing(16)], [
      section_title("Virtual keyboard"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Each key below is a plain Emerge tree node using Event.virtual_key/1. Focus the text input for letters, or focus the key listener pad to watch Enter and arrows hit on_key_* handlers. Hold A or E to open alternates, and hold Backspace or arrows to repeat."
        )
      ),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(46, 50, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            soft_status_chip(
              "Target",
              soft_keyboard_target_label(),
              color_rgb(70, 82, 112),
              color_rgb(226, 234, 248)
            ),
            soft_status_chip(
              "Shift",
              if(shift_active, do: "On", else: "Off"),
              if(shift_active, do: color_rgb(88, 118, 78), else: color_rgb(76, 74, 102)),
              if(shift_active,
                do: color_rgb(230, 246, 228),
                else: color_rgb(224, 228, 242)
              )
            ),
            soft_status_chip(
              "Popup",
              popup_label,
              color_rgb(92, 76, 116),
              color_rgb(244, 232, 248)
            ),
            soft_status_chip(
              "Hold events",
              Integer.to_string(hold_count),
              color_rgb(74, 90, 124),
              color_rgb(230, 238, 250)
            )
          ]),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            soft_shift_toggle_key(shift_active),
            soft_letter_key(:a, "a", "A", :a, shift_active, popup: popup),
            soft_letter_key(:e, "e", "E", :e, shift_active, popup: popup),
            soft_letter_key(:i, "i", "I", :i, shift_active),
            soft_letter_key(:o, "o", "O", :o, shift_active),
            soft_letter_key(:u, "u", "U", :u, shift_active),
            soft_special_key_button(
              "Backspace",
              [tap: {:key, :backspace, []}, hold: :repeat],
              id: :backspace,
              width: px(124),
              background: color_rgb(88, 68, 84),
              hover_background: color_rgb(104, 82, 98),
              border_color: color_rgb(168, 126, 152)
            )
          ]),
          wrapped_row([width(fill()), spacing_xy(8, 8)], [
            soft_special_key_button(
              "Left",
              [tap: {:key, :arrow_left, []}, hold: :repeat],
              id: :arrow_left,
              width: px(92),
              background: color_rgb(62, 84, 108),
              hover_background: color_rgb(76, 98, 124),
              border_color: color_rgb(138, 170, 204)
            ),
            soft_special_key_button(
              "Space",
              [tap: {:text_and_key, " ", :space, []}],
              id: :space,
              width: px(220),
              background: color_rgb(64, 72, 110),
              hover_background: color_rgb(78, 86, 126),
              border_color: color_rgb(138, 150, 206)
            ),
            soft_special_key_button(
              "Right",
              [tap: {:key, :arrow_right, []}, hold: :repeat],
              id: :arrow_right,
              width: px(92),
              background: color_rgb(62, 84, 108),
              hover_background: color_rgb(76, 98, 124),
              border_color: color_rgb(138, 170, 204)
            ),
            soft_special_key_button(
              "Enter",
              [tap: {:key, :enter, []}],
              id: :enter,
              width: px(110),
              background: color_rgb(72, 92, 78),
              hover_background: color_rgb(86, 108, 92),
              border_color: color_rgb(156, 196, 164)
            )
          ]),
          el(
            [Font.size(11), Font.color(color_rgb(230, 234, 246))],
            text("Soft keyboard status: #{last_action}")
          ),
          el(
            [Font.size(10), Font.color(@dim_text)],
            text(
              "Try this flow: focus the text field, tap Shift, tap A, hold E for alternates, then tap an accented popup key. Focus the keyboard listener pad to route Enter and arrows into on_key_* counters instead."
            )
          )
        ])
      )
    ])
  end

  defp soft_keyboard_target_label() do
    cond do
      Process.get(:demo_input_focused, false) -> "Text input"
      Process.get(:demo_key_listener_focused, false) -> "Keyboard listener pad"
      Process.get(:demo_button_focused, false) -> "Run action button"
      true -> "Nothing focused"
    end
  end

  defp soft_status_chip(label, value, bg_color, text_color) do
    el(
      [padding_xy(10, 5), Background.color(bg_color), Border.rounded(999)],
      el([Font.size(11), Font.color(text_color)], text("#{label}: #{value}"))
    )
  end

  defp soft_shift_toggle_key(active?) do
    bg = if active?, do: color_rgb(92, 118, 76), else: color_rgb(64, 68, 98)
    hover_bg = if active?, do: color_rgb(106, 132, 88), else: color_rgb(76, 82, 112)
    border_color = if active?, do: color_rgb(168, 204, 144), else: color_rgb(132, 140, 188)
    label = if active?, do: "Shift On", else: "Shift"

    el(
      [
        key(:soft_shift_toggle),
        width(px(96)),
        padding_xy(12, 10),
        Background.color(bg),
        Border.rounded(10),
        Border.width(1),
        Border.color(border_color),
        Font.size(12),
        Font.color(:white),
        Event.on_click({self(), {:demo_event, :soft_keyboard, :toggle_shift}}),
        Interactive.mouse_over([
          Background.color(hover_bg),
          Border.color(color_rgb(196, 208, 238))
        ]),
        Interactive.mouse_down([
          Background.color(color_rgb(58, 62, 90)),
          Transform.move_y(1)
        ])
      ],
      text(label)
    )
  end

  defp soft_letter_key(letter, lower, upper, key_name, shift_active, opts \\ []) do
    popup_key = Keyword.get(opts, :popup)

    popup_content =
      if popup_key == letter, do: soft_alternate_popup(letter, shift_active), else: nil

    spec =
      [
        tap:
          if(shift_active,
            do: {:text_and_key, upper, key_name, [:shift]},
            else: {:text_and_key, lower, key_name, []}
          )
      ] ++
        if letter in [:a, :e] do
          [hold: {:event, {self(), {:demo_event, :soft_keyboard, {:show_alternates, letter}}}}]
        else
          []
        end

    soft_special_key_button(
      if(shift_active, do: upper, else: lower),
      spec,
      id: {:letter, letter},
      width: px(56),
      popup: popup_content,
      background: color_rgb(68, 74, 108),
      hover_background: color_rgb(82, 88, 126),
      border_color: color_rgb(142, 152, 206)
    )
  end

  defp soft_special_key_button(label, spec, opts) do
    background = Keyword.get(opts, :background, color_rgb(68, 74, 108))
    hover_background = Keyword.get(opts, :hover_background, color_rgb(82, 88, 126))
    border_color = Keyword.get(opts, :border_color, color_rgb(142, 152, 206))
    popup = Keyword.get(opts, :popup)

    attrs = [
      key({:soft_key, Keyword.fetch!(opts, :id)}),
      width(Keyword.get(opts, :width, px(64))),
      padding_xy(12, 10),
      Background.color(background),
      Border.rounded(10),
      Border.width(1),
      Border.color(border_color),
      Font.size(12),
      Font.color(:white),
      Event.virtual_key(spec),
      Interactive.mouse_over([
        Background.color(hover_background),
        Border.color(color_rgb(196, 208, 238))
      ]),
      Interactive.mouse_down([
        Background.color(color_rgb(58, 62, 90)),
        Transform.move_y(1)
      ])
    ]

    attrs = if popup, do: [Nearby.above(popup) | attrs], else: attrs

    el(attrs, text(label))
  end

  defp soft_alternate_popup(:a, shift_active) do
    labels = if shift_active, do: ["Á", "À", "Ä"], else: ["á", "à", "ä"]
    soft_alternate_popup_panel(:a, labels)
  end

  defp soft_alternate_popup(:e, shift_active) do
    labels = if shift_active, do: ["É", "È", "Ê"], else: ["é", "è", "ê"]
    soft_alternate_popup_panel(:e, labels)
  end

  defp soft_alternate_popup_panel(owner, labels) do
    el(
      [
        key({:soft_popup, owner}),
        padding(8),
        Background.color(color_rgb(38, 42, 64)),
        Border.rounded(12),
        Border.width(1),
        Border.color(color_rgb(154, 168, 224))
      ],
      row(
        [spacing(6)],
        Enum.map(labels, fn label ->
          soft_special_key_button(
            label,
            [tap: {:text, label}],
            id: {:popup_key, owner, label},
            width: px(48),
            background: color_rgb(82, 72, 118),
            hover_background: color_rgb(98, 84, 134),
            border_color: color_rgb(188, 170, 236)
          )
        end)
      )
    )
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
            Background.color(color_rgb(55, 70, 90)),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Shrink")),
            el([Font.size(11), Font.color(color_rgb(210, 220, 230))], text("Content sized"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(10),
            Background.color(color_rgb(70, 80, 95)),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Fill")),
            el([Font.size(11), Font.color(color_rgb(220, 225, 235))], text("Expands"))
          ])
        )
      ]),
      row([width(fill()), spacing(8)], [
        el(
          [
            width({:fill, 1}),
            padding(8),
            Background.color(color_rgb(65, 70, 100)),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 1"))
        ),
        el(
          [
            width({:fill, 2}),
            padding(8),
            Background.color(color_rgb(65, 80, 110)),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 2"))
        ),
        el(
          [
            width({:fill, 3}),
            padding(8),
            Background.color(color_rgb(65, 90, 120)),
            Border.rounded(8)
          ],
          el([Font.size(12), Font.color(:white)], text("Fill 3"))
        )
      ]),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(min(px(140), shrink())),
            padding(10),
            Background.color(color_rgb(70, 65, 95)),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Min + shrink")),
            el([Font.size(11), Font.color(color_rgb(220, 220, 235))], text(">= 140px"))
          ])
        ),
        el(
          [
            width(max(px(180), fill())),
            padding(10),
            Background.color(color_rgb(85, 65, 95)),
            Border.rounded(8)
          ],
          column([spacing(4)], [
            el([Font.size(13), Font.color(:white)], text("Max + fill")),
            el([Font.size(11), Font.color(color_rgb(225, 215, 235))], text("<= 180px"))
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
          Background.color(color_rgb(45, 45, 65)),
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
          Background.color(color_rgb(45, 45, 65)),
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
          Background.color(color_rgb(45, 45, 65)),
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
    swipe_last = Process.get(:demo_swipe_last_direction, "None")
    swipe_up_count = Process.get(:demo_swipe_up_count, 0)
    swipe_down_count = Process.get(:demo_swipe_down_count, 0)
    swipe_left_count = Process.get(:demo_swipe_left_count, 0)
    swipe_right_count = Process.get(:demo_swipe_right_count, 0)

    column([width(fill()), spacing(16)], [
      section_title("Mouse Events"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Hover, press, and move inside the cards.")
      ),
      row([width(fill()), spacing(12)], [
        event_card("Mouse Down", :mouse_down, color_rgb(70, 70, 110)),
        event_card("Mouse Up", :mouse_up, color_rgb(70, 90, 90))
      ]),
      row([width(fill()), spacing(12)], [
        event_card("Mouse Enter", :mouse_enter, color_rgb(85, 65, 100)),
        event_card("Mouse Leave", :mouse_leave, color_rgb(90, 70, 60))
      ]),
      event_card("Mouse Move", :mouse_move, color_rgb(60, 80, 110)),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text("Last move target: #{move_label}")
      ),
      section_title("Swipe Gestures"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Press, drag past the deadzone, and release. The final net displacement on release chooses the swipe direction."
        )
      ),
      swipe_showcase_panel(
        swipe_last,
        swipe_up_count,
        swipe_down_count,
        swipe_left_count,
        swipe_right_count
      ),
      section_title("Transformed Hit Testing"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "The faint outline shows the original slot. Pointer events should follow the transformed card you see in front."
        )
      ),
      wrapped_row([width(fill()), spacing_xy(14, 14)], [
        transformed_event_showcase(
          "Translated Move",
          "Transform.move_x(40), Transform.move_y(14)",
          "Hover glow and move tracking both follow the shifted card.",
          [Transform.move_x(40), Transform.move_y(14)],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(92, 120, 176)),
              Border.color(color_rgb(190, 216, 255)),
              Border.glow(color_rgba(110, 160, 255, 90 / 255), 2),
              Font.color(:white)
            ])
          ],
          [:mouse_move],
          color_rgb(68, 92, 138)
        ),
        transformed_event_showcase(
          "Rotated Hover",
          "Transform.rotate(16)",
          "Enter, leave, and hover styling all follow the painted angle.",
          [Transform.rotate(16)],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(128, 94, 162)),
              Border.color(color_rgb(228, 198, 255)),
              Border.glow(color_rgba(196, 132, 255, 92 / 255), 2),
              Font.color(:white)
            ])
          ],
          [:mouse_enter, :mouse_leave],
          color_rgb(100, 74, 126)
        ),
        transformed_event_showcase(
          "Scaled Press",
          "Transform.scale(1.18)",
          "Hover glow and mouse_down inset both stay on the scaled shape.",
          [Transform.scale(1.18)],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(104, 124, 96)),
              Border.color(color_rgb(220, 236, 204)),
              Border.glow(color_rgba(160, 212, 136, 82 / 255), 2),
              Font.color(:white)
            ]),
            Interactive.mouse_down([
              Background.color(color_rgb(72, 88, 64)),
              Border.color(color_rgb(214, 228, 194)),
              Border.inner_shadow(
                offset: {0, 1},
                blur: 6,
                size: 1,
                color: color_rgba(0, 0, 0, 120 / 255)
              ),
              Font.color(:white)
            ])
          ],
          [:mouse_down, :mouse_up],
          color_rgb(86, 104, 78)
        )
      ])
    ])
  end

  defp swipe_showcase_panel(last_swipe, up_count, down_count, left_count, right_count) do
    column([width(fill()), spacing(10)], [
      wrapped_row([width(fill()), spacing_xy(8, 8)], [
        swipe_stat_pill(
          "Last",
          last_swipe,
          color_rgb(76, 72, 118),
          color_rgb(242, 236, 252)
        ),
        swipe_stat_pill("Up", up_count, color_rgb(72, 96, 132), color_rgb(232, 242, 252)),
        swipe_stat_pill(
          "Down",
          down_count,
          color_rgb(84, 78, 130),
          color_rgb(240, 236, 252)
        ),
        swipe_stat_pill(
          "Left",
          left_count,
          color_rgb(72, 106, 110),
          color_rgb(228, 246, 246)
        ),
        swipe_stat_pill(
          "Right",
          right_count,
          color_rgb(104, 84, 124),
          color_rgb(246, 236, 248)
        )
      ]),
      el(
        [
          width(fill()),
          padding(14),
          spacing(10),
          Background.color(color_rgb(46, 50, 72)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          el(
            [Font.size(11), Font.color(@dim_text)],
            text(
              "This pad is intentionally not scrollable, so drag falls through to swipe. Scroll containers still win first."
            )
          ),
          el(
            [
              width(fill()),
              height(px(240)),
              padding(14),
              Background.color(color_rgb(62, 66, 96)),
              Border.rounded(14),
              Border.width(1),
              Border.color(color_rgb(118, 128, 178)),
              Event.on_swipe_up({self(), {:demo_event, :swipe_showcase, :up}}),
              Event.on_swipe_down({self(), {:demo_event, :swipe_showcase, :down}}),
              Event.on_swipe_left({self(), {:demo_event, :swipe_showcase, :left}}),
              Event.on_swipe_right({self(), {:demo_event, :swipe_showcase, :right}}),
              Interactive.mouse_over([
                Background.color(color_rgb(70, 76, 108)),
                Border.color(color_rgb(160, 178, 236)),
                Border.glow(color_rgba(132, 158, 232, 84 / 255), 2)
              ]),
              Interactive.mouse_down([
                Background.color(color_rgb(56, 62, 88)),
                Border.color(color_rgb(226, 192, 132)),
                Transform.move_y(1)
              ])
            ],
            column([width(fill()), height(fill()), space_evenly()], [
              el(
                [center_x(), Font.size(10), Font.color(color_rgb(214, 226, 246))],
                text("Swipe up")
              ),
              row([width(fill()), space_evenly()], [
                el([Font.size(10), Font.color(color_rgb(214, 226, 246))], text("Swipe left")),
                column([center_x(), center_y(), spacing(6)], [
                  el([Font.size(18), Font.color(:white)], text("Swipe Pad")),
                  el(
                    [Font.size(11), Font.color(color_rgb(216, 222, 242))],
                    text("Release decides direction")
                  ),
                  el(
                    [Font.size(10), Font.color(color_rgb(198, 206, 232))],
                    text("Quick drag + release")
                  )
                ]),
                el(
                  [Font.size(10), Font.color(color_rgb(214, 226, 246))],
                  text("Swipe right")
                )
              ]),
              el(
                [center_x(), Font.size(10), Font.color(color_rgb(214, 226, 246))],
                text("Swipe down")
              )
            ])
          ),
          el(
            [Font.size(10), Font.color(@dim_text)],
            text(
              "Counters update after release. Short drags and balanced diagonals are ignored so the demo does not misfire on casual movement."
            )
          )
        ])
      )
    ])
  end

  defp swipe_stat_pill(label, value, bg, fg) do
    el(
      [padding_xy(10, 5), Background.color(bg), Border.rounded(999)],
      el([Font.size(11), Font.color(fg)], text("#{label}: #{value}"))
    )
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
    bg = if active, do: color_rgb(88, 72, 122), else: color_rgb(58, 52, 82)
    border = if active, do: color_rgb(188, 154, 250), else: color_rgb(120, 112, 150)
    title_color = if active, do: @light_text, else: color_rgb(220, 210, 240)
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
          Transform.move_y(if(active, do: -2, else: 0)),
          Event.on_mouse_enter({self(), {:demo_event, :hover_manual, :mouse_enter}}),
          Event.on_mouse_leave({self(), {:demo_event, :hover_manual, :mouse_leave}})
        ],
        column([spacing(6)], [
          el([Font.size(13), Font.color(title_color)], text("Event-managed hover")),
          el([Font.size(11), Font.color(title_color)], text(state_text)),
          el(
            [Font.size(10), Font.color(color_rgb(225, 215, 245))],
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
          Background.color(color_rgb(52, 70, 84)),
          Border.rounded(10),
          Border.width(1),
          Border.color(color_rgb(102, 124, 150)),
          Interactive.mouse_over([
            Background.color(color_rgb(86, 112, 140)),
            Border.color(color_rgb(168, 210, 250)),
            Font.color(@light_text),
            Font.underline(),
            Font.strike(),
            Font.letter_spacing(1.4),
            Font.word_spacing(2.5),
            Transform.move_y(-2),
            Transform.scale(1.02)
          ])
        ],
        column([spacing(6)], [
          el(
            [Font.size(13), Font.color(color_rgb(210, 222, 240))],
            text("Declarative hover style")
          ),
          el(
            [Font.size(11), Font.color(color_rgb(190, 206, 228))],
            text("No enter/leave handlers or hover state in Elixir.")
          ),
          el(
            [Font.size(10), Font.color(color_rgb(214, 228, 246))],
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
          Event.on_click({self(), :scramble_unstable})
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
            Background.color(color_rgb(50, 50, 75)),
            Border.rounded(8),
            spacing(8),
            Event.on_click({self(), {:unstable_row_click, item.label}})
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
                Background.color(color_rgb(40, 40, 60)),
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
                      Background.color(color_rgb(70, 70, 95)),
                      Border.rounded(10),
                      Event.on_click({self(), {:unstable_child_click, item.label, child.label}})
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
            Background.color(color_rgb(55, 55, 80)),
            Border.rounded(4),
            Font.size(12),
            Font.color(:white)
          ],
          text("Left")
        ),
        el(
          [
            padding(10),
            Background.color(color_rgb(55, 55, 80)),
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
            Background.color(color_rgb(55, 55, 80)),
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
            Background.color(color_rgb(55, 55, 80)),
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
            Background.color(color_rgb(55, 55, 80)),
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
            Background.color(color_rgb(55, 55, 80)),
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
            Background.color(color_rgb(55, 55, 80)),
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
          Background.color(color_rgb(45, 45, 65)),
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
      column(
        [
          width(fill()),
          Border.rounded(12),
          width(px(365)),
          Background.color(color_rgb(255, 255, 255))
        ],
        [forecast_now(), forecast_week()]
      )
    ])
  end

  defp forecast_now() do
    row(
      [
        width(fill()),
        Background.color(color_rgb(240, 237, 248)),
        padding(16)
      ],
      [
        el(
          [width(fill()), Border.width(1), Border.color(color_rgb(0, 0, 0))],
          column(
            [center_x(), Font.color(color_rgb(26, 31, 39)), Font.bold(), spacing(10)],
            [
              row([spacing(16)], [text("CL"), text("68°")]),
              row([], [text("Partly Cloudy")])
            ]
          )
        ),
        el(
          [width(fill()), Border.width(1), Border.color(color_rgb(0, 0, 0))],
          column(
            [
              center_x(),
              Font.color(color_rgb(26, 31, 39)),
              Font.bold(),
              spacing(10)
            ],
            [
              row([spacing(16)], [text("65%"), text("WI"), text("8 mph")]),
              row([spacing(12)], [text("H: 72°"), text("L: 63°")])
            ]
          )
        )
      ]
    )
  end

  defp forecast_week() do
    row([width(fill()), height(px(120))], [])
  end

  defp page_transforms() do
    column([width(fill()), spacing(16)], [
      section_title("Transforms"),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(14),
            Background.color(color_rgb(50, 70, 90)),
            Border.rounded(10),
            Transform.rotate(-8),
            Transform.alpha(0.8)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Rotate")),
            el([Font.size(11), Font.color(color_rgb(200, 220, 230))], text("-8deg"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(14),
            Background.color(color_rgb(70, 60, 90)),
            Border.rounded(10),
            Transform.scale(1.08)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Scale")),
            el([Font.size(11), Font.color(color_rgb(220, 210, 235))], text("1.08x"))
          ])
        )
      ]),
      row([width(fill()), spacing(12)], [
        el(
          [
            width(fill()),
            padding(14),
            Background.color(color_rgb(60, 80, 70)),
            Border.rounded(10),
            Transform.move_x(16)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Move")),
            el([Font.size(11), Font.color(color_rgb(210, 230, 220))], text("+16px x"))
          ])
        ),
        el(
          [
            width(fill()),
            padding(14),
            Background.color(color_rgb(80, 70, 60)),
            Border.rounded(10),
            Transform.alpha(0.6)
          ],
          column([spacing(4)], [
            el([Font.size(14), Font.color(:white)], text("Alpha")),
            el([Font.size(11), Font.color(color_rgb(230, 220, 210))], text("60%"))
          ])
        )
      ])
    ])
  end

  defp page_animation() do
    column([width(fill()), spacing(16)], [
      section_title("Animation"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "These cards animate layout and transform attrs continuously. Pointer events and state styles should stay attached to the painted shape, not the original slot."
        )
      ),
      section_title("Animated Layout + Hit Testing"),
      wrapped_row([width(fill()), spacing_xy(14, 14)], [
        transformed_event_showcase(
          "Animated Width + Move",
          "width(px(96 -> 156)) + Transform.move_x(-16 -> 26)",
          "Move across the card while it shifts and resizes; hit testing follows the painted shape.",
          [
            Animation.animate(
              [
                [width(px(96)), Transform.move_x(-16), Transform.move_y(0), Transform.rotate(0)],
                [
                  width(px(156)),
                  Transform.move_x(26),
                  Transform.move_y(-10),
                  Transform.rotate(-45)
                ]
              ],
              1400,
              :ease_in_out,
              :loop
            )
          ],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(96, 132, 188)),
              Border.color(color_rgb(200, 224, 255)),
              Border.glow(color_rgba(110, 160, 255, 92 / 255), 2),
              Font.color(:white)
            ])
          ],
          [:mouse_move],
          color_rgb(70, 96, 148)
        ),
        transformed_event_showcase(
          "Animated Padding + Rotate",
          "padding_each(...) + Transform.rotate(-10 -> 10)",
          "Enter and leave the card while its padding and angle animate; hover stays visually aligned.",
          [
            Animation.animate(
              [
                [padding_each(10, 12, 10, 12), Transform.rotate(-10)],
                [padding_each(18, 24, 18, 24), Transform.rotate(10)]
              ],
              1600,
              :ease_in_out,
              :loop
            )
          ],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(130, 96, 166)),
              Border.color(color_rgb(232, 204, 255)),
              Border.glow(color_rgba(196, 132, 255, 92 / 255), 2),
              Font.color(:white)
            ])
          ],
          [:mouse_enter, :mouse_leave],
          color_rgb(102, 76, 130)
        ),
        transformed_event_showcase(
          "Animated Height + Scale Press",
          "height(px(70 -> 108)) + Transform.scale(0.94 -> 1.08)",
          "Press the card while its height and scale animate; mouse_down styling still tracks the visible shape.",
          [
            Animation.animate(
              [
                [height(px(70)), Transform.scale(0.94)],
                [height(px(108)), Transform.scale(1.08)]
              ],
              1200,
              :ease_in_out,
              :loop
            )
          ],
          [
            Interactive.mouse_over([
              Background.color(color_rgb(106, 126, 98)),
              Border.color(color_rgb(220, 236, 204)),
              Border.glow(color_rgba(160, 212, 136, 84 / 255), 2),
              Font.color(:white)
            ]),
            Interactive.mouse_down([
              Background.color(color_rgb(74, 90, 66)),
              Border.color(color_rgb(214, 228, 194)),
              Border.inner_shadow(
                offset: {0, 1},
                blur: 6,
                size: 1,
                color: color_rgba(0, 0, 0, 120 / 255)
              ),
              Font.color(:white)
            ])
          ],
          [:mouse_down, :mouse_up],
          color_rgb(86, 104, 78)
        )
      ]),
      section_title("Enter Animation"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "This shelf is conditionally inserted on click. Opening mounts it with animate_enter; closing removes the live node immediately and lets animate_exit finish as a passive ghost."
        )
      ),
      enter_shelf_showcase()
    ])
  end

  defp page_nearby() do
    column([width(fill()), spacing(16)], [
      section_title("Nearby Elements"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "Nearby roots align from the host slot; in_front uses the host border-box and explicit px sizes can overflow."
        )
      ),
      el(
        [
          width(fill()),
          height(px(160)),
          padding(15),
          Background.color(color_rgba(45, 45, 65, 40 / 255)),
          Border.rounded(6)
        ],
        el(
          [
            width(px(140)),
            height(px(60)),
            center_x(),
            center_y(),
            Background.color(color_rgb(70, 70, 120)),
            Border.rounded(6),
            Nearby.above(
              el(
                [
                  padding(6),
                  Background.color(color_rgb(90, 70, 70)),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Above")
              )
            ),
            Nearby.below(
              el(
                [
                  padding(6),
                  Background.color(color_rgb(70, 90, 70)),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Below")
              )
            ),
            Nearby.on_left(
              el(
                [
                  padding(6),
                  Background.color(color_rgb(70, 70, 90)),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Left")
              )
            ),
            Nearby.on_right(
              el(
                [
                  padding(6),
                  Background.color(color_rgb(90, 90, 70)),
                  Border.rounded(4),
                  Font.size(12),
                  Font.color(:white)
                ],
                text("Right")
              )
            ),
            Nearby.behind_content(
              el(
                [
                  width(px(160)),
                  height(px(70)),
                  Background.color(color_rgba(200, 200, 255, 40 / 255)),
                  Border.rounded(8)
                ],
                text("Behind")
              )
            ),
            Nearby.in_front(
              el(
                [
                  padding(4),
                  Background.color(color_rgba(0, 0, 0, 120 / 255)),
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
      ),
      section_title("Oversized inFront"),
      el(
        [Font.size(12), Font.color(@dim_text)],
        text(
          "The same 220x90 overlay is centered and bottom-aligned inside a 126x78 host. Clipping only changes visibility."
        )
      ),
      nearby_overflow_card(
        "Escapes host bounds",
        "The overlay spills past the border-box slot and still paints above the host."
      )
    ])
  end

  defp nearby_overflow_card(title, note) do
    host_attrs = [
      width(px(126)),
      height(px(78)),
      center_x(),
      center_y(),
      Background.color(color_rgb(76, 76, 132)),
      Border.width(2),
      Border.color(color_rgb(182, 194, 255)),
      Border.rounded(10),
      Nearby.in_front(nearby_oversized_front_overlay())
    ]

    column(
      [
        width(fill()),
        spacing(10),
        padding(12),
        Background.color(color_rgb(45, 45, 65)),
        Border.rounded(8)
      ],
      [
        el([Font.size(13), Font.color(@light_text)], text(title)),
        el([Font.size(11), Font.color(@dim_text)], text(note)),
        el(
          [
            width(fill()),
            height(px(180)),
            Background.color(color_rgba(255, 255, 255, 12 / 255)),
            Border.rounded(8)
          ],
          el(
            host_attrs,
            el([center_x(), center_y(), Font.size(13), Font.color(:white)], text("Host"))
          )
        )
      ]
    )
  end

  defp nearby_oversized_front_overlay() do
    el(
      [
        width(px(220)),
        height(px(90)),
        center_x(),
        align_bottom(),
        Background.color(color_rgba(235, 96, 140, 210 / 255)),
        Border.rounded(10),
        Font.size(11),
        Font.color(:white)
      ],
      column([center_x(), center_y(), spacing(4)], [
        text("in_front 220x90"),
        el(
          [Font.size(10), Font.color(color_rgba(255, 255, 255, 220 / 255))],
          text("center_x + align_bottom")
        )
      ])
    )
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
          Font.color(color_rgb(200, 220, 255)),
          Background.color(color_rgb(45, 45, 65)),
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
            Background.color(color_rgb(54, 70, 90)),
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
            Background.color(color_rgb(72, 62, 88)),
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
            Background.color(color_rgb(70, 80, 62)),
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
            Background.color(color_rgb(45, 60, 82)),
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
            Background.color(color_rgb(56, 74, 66)),
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
            Background.color(color_rgb(75, 62, 62)),
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
            Background.color(color_rgb(55, 55, 80)),
            Border.rounded(4)
          ],
          el([width(fill()), Font.size(12), Font.color(:white), Font.align_left()], text("Left"))
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color(color_rgb(55, 55, 80)),
            Border.rounded(4)
          ],
          el([width(fill()), Font.size(12), Font.color(:white), Font.center()], text("Center"))
        ),
        el(
          [
            width(fill()),
            padding(8),
            Background.color(color_rgb(55, 55, 80)),
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
          Font.color(color_rgb(180, 180, 200)),
          Background.color(color_rgb(50, 50, 70)),
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
          Background.color(color_rgb(45, 45, 65)),
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
          Background.color(color_rgb(45, 45, 65)),
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
              Background.color(color_rgb(45, 45, 65)),
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
              Background.color(color_rgb(45, 45, 65)),
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
          Background.color(color_rgb(40, 40, 60)),
          Border.rounded(10)
        ],
        column([width(fill()), spacing(12)], [
          el([Font.size(20), Font.bold(), Font.color(:white)], text("Getting Started")),
          paragraph([spacing(4), Font.size(14), Font.color(color_rgb(210, 210, 230))], [
            text(
              "Emerge is a native GUI toolkit for Elixir that renders with Skia. " <>
                "It uses a declarative layout model inspired by elm-ui, where you describe " <>
                "what your interface should look like and the engine handles the rest."
            )
          ]),
          paragraph([spacing(4), Font.size(14), Font.color(color_rgb(210, 210, 230))], [
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
          Background.color(color_rgb(37, 44, 58)),
          Border.rounded(10)
        ],
        text_column(
          [
            center_x(),
            spacing(14),
            Font.size(14),
            Font.color(color_rgb(220, 226, 236))
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
          Background.color(color_rgb(44, 50, 66)),
          Border.rounded(10)
        ],
        paragraph([spacing(4), Font.size(14), Font.color(color_rgb(226, 232, 243))], [
          el(
            [
              align_left(),
              width(px(40)),
              height(px(40)),
              Background.color(color_rgb(74, 113, 214)),
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
              Background.color(color_rgb(78, 58, 90)),
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
          Background.color(color_rgb(36, 46, 60)),
          Border.rounded(10)
        ],
        text_column(
          [spacing(10), Font.size(13), Font.color(color_rgb(220, 228, 238))],
          [
            el(
              [
                align_left(),
                width(px(128)),
                height(px(92)),
                padding(10),
                Background.color(color_rgb(67, 97, 150)),
                Border.rounded(8)
              ],
              column([spacing(6)], [
                el([Font.bold(), Font.color(:white)], text("Floated Card")),
                el(
                  [Font.size(11), Font.color(color_rgb(232, 238, 246))],
                  text("align_left()")
                ),
                el([Font.size(11), Font.color(color_rgb(232, 238, 246))], text("92px tall"))
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
                Background.color(color_rgb(84, 62, 62)),
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
        el([Font.size(10), Font.color(color_rgb(200, 200, 220))], text("Border attrs:")),
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
      [center_x(), width(max(px(max_width), fill()))],
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
        Background.color(color_rgb(50, 50, 74)),
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
            Background.color(color_rgb(34, 34, 50)),
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
        Background.color(color_rgb(50, 50, 74)),
        Border.rounded(10)
      ],
      column([spacing(8)], [
        row([width(fill()), spacing(8)], [
          el([width(fill()), Font.size(12), Font.color(:white)], text(title)),
          source_status_chip(status_label, status_tone)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(source_label)),
        el([Font.size(10), Font.color(color_rgb(196, 202, 222))], text(note)),
        el(
          [
            width(fill()),
            padding(10),
            Background.color(color_rgb(34, 34, 50)),
            Border.rounded(8)
          ],
          column([spacing(6)], [
            el([Font.size(22), Font.color(:white)] ++ font_attrs, text("Asset Fonts 123")),
            el(
              [Font.size(12), Font.color(color_rgb(214, 220, 236))] ++ font_attrs,
              text(sample)
            )
          ])
        )
      ])
    )
  end

  defp weather_forecast_data() do
    [
      %{day: "Mon", kind: :sun, high_c: 22, low_c: 13, precip: "5%"},
      %{day: "Tue", kind: :cloud, high_c: 19, low_c: 12, precip: "20%"},
      %{day: "Wed", kind: :rain, high_c: 16, low_c: 10, precip: "70%"},
      %{day: "Thu", kind: :cloud, high_c: 18, low_c: 11, precip: "25%"},
      %{day: "Fri", kind: :sun, high_c: 24, low_c: 14, precip: "5%"},
      %{day: "Sat", kind: :rain, high_c: 17, low_c: 9, precip: "80%"},
      %{day: "Sun", kind: :sun, high_c: 23, low_c: 13, precip: "10%"}
    ]
  end

  defp weather_widget_card(days) do
    counts = Enum.frequencies_by(days, & &1.kind)

    summary =
      "#{Map.get(counts, :sun, 0)} sunny, #{Map.get(counts, :cloud, 0)} cloudy, #{Map.get(counts, :rain, 0)} rainy across the week"

    el(
      [
        width(fill()),
        padding(16),
        spacing(14),
        Background.gradient(color_rgb(16, 52, 102), color_rgb(44, 132, 182), 90),
        Border.rounded(18),
        Border.width(1),
        Border.color(color_rgba(204, 233, 255, 120 / 255)),
        Border.glow(color_rgba(66, 156, 230, 90 / 255), 4)
      ],
      column([spacing(14)], [
        row([width(fill()), spacing(12)], [
          column([width(fill()), spacing(6)], [
            el([Font.size(22), Font.color(:white)], text("Weekly forecast")),
            el(
              [Font.size(12), Font.color(color_rgb(226, 238, 249))],
              text("North Shore boardwalk · local SVG weather icons rendered with svg/2")
            ),
            row([width(fill()), spacing(8)], [
              weather_badge("SVG via svg/2", color_rgba(5, 20, 34, 105 / 255)),
              weather_badge("C primary", color_rgba(28, 83, 49, 140 / 255)),
              weather_badge("F secondary", color_rgba(64, 52, 20, 130 / 255))
            ])
          ]),
          column([spacing(8)], [
            el(
              [
                padding_each(5, 10, 5, 10),
                Background.color(color_rgba(6, 24, 40, 110 / 255)),
                Border.rounded(999),
                Font.size(11),
                Font.color(:white)
              ],
              text("Hardcoded sample")
            ),
            el(
              [Font.size(11), Font.color(color_rgb(220, 236, 248))],
              text(summary)
            )
          ])
        ]),
        el(
          [
            width(fill()),
            padding(10),
            Background.color(color_rgba(5, 20, 34, 95 / 255)),
            Border.rounded(14)
          ],
          wrapped_row([width(fill()), spacing_xy(10, 10)], Enum.map(days, &weather_day_card/1))
        )
      ])
    )
  end

  defp weather_day_card(%{day: day, kind: kind, high_c: high_c, low_c: low_c, precip: precip}) do
    {icon_glow, accent, detail_bg} = weather_icon_tone(kind)

    el(
      [
        width(px(118)),
        padding(10),
        spacing(8),
        Background.color(color_rgba(8, 18, 30, 145 / 255)),
        Border.rounded(14),
        Border.width(1),
        Border.color(color_rgba(226, 238, 248, 60 / 255))
      ],
      column([center_x(), spacing(8)], [
        el([Font.size(12), Font.color(:white)], text(day)),
        el(
          [
            width(px(58)),
            height(px(58)),
            padding(8),
            Background.color(icon_glow),
            Border.rounded(999)
          ],
          svg(
            [width(fill()), height(fill()), image_fit(:contain)],
            weather_icon_source(kind)
          )
        ),
        el(
          [Font.size(11), Font.color(color_rgb(232, 238, 248))],
          text(weather_condition_label(kind))
        ),
        weather_temp_line("HI", high_c, accent),
        weather_temp_line("LO", low_c, color_rgb(214, 223, 236)),
        el(
          [
            padding_each(3, 8, 3, 8),
            Background.color(detail_bg),
            Border.rounded(999),
            Font.size(9),
            Font.color(:white)
          ],
          text("precip #{precip}")
        )
      ])
    )
  end

  defp weather_temp_line(label, temp_c, primary_color) do
    row([center_x(), spacing(6)], [
      el([Font.size(9), Font.color(@dim_text)], text(label)),
      el([Font.size(15), Font.color(primary_color)], text("#{temp_c}C")),
      el(
        [Font.size(11), Font.color(color_rgb(218, 226, 239))],
        text("#{celsius_to_fahrenheit(temp_c)}F")
      )
    ])
  end

  defp weather_badge(label, bg_color) do
    el(
      [
        padding_each(4, 8, 4, 8),
        Background.color(bg_color),
        Border.rounded(999),
        Font.size(10),
        Font.color(:white)
      ],
      text(label)
    )
  end

  defp weather_icon_source(:sun), do: "demo_images/weather_sun.svg"
  defp weather_icon_source(:cloud), do: "demo_images/weather_cloud.svg"
  defp weather_icon_source(:rain), do: "demo_images/weather_rain.svg"

  defp weather_condition_label(:sun), do: "Sunny"
  defp weather_condition_label(:cloud), do: "Cloudy"
  defp weather_condition_label(:rain), do: "Rain"

  defp weather_icon_tone(:sun) do
    {
      color_rgba(255, 209, 102, 34 / 255),
      color_rgb(255, 215, 110),
      color_rgba(102, 74, 18, 170 / 255)
    }
  end

  defp weather_icon_tone(:cloud) do
    {
      color_rgba(198, 212, 233, 34 / 255),
      color_rgb(224, 232, 243),
      color_rgba(65, 84, 108, 170 / 255)
    }
  end

  defp weather_icon_tone(:rain) do
    {
      color_rgba(110, 198, 255, 34 / 255),
      color_rgb(136, 224, 255),
      color_rgba(32, 88, 114, 175 / 255)
    }
  end

  defp celsius_to_fahrenheit(temp_c) do
    round(temp_c * 9 / 5 + 32)
  end

  defp svg_weather_scale_showcase(specs) do
    cards =
      Enum.map(specs, fn {label, note, source} -> svg_weather_scale_card(label, note, source) end)

    column([spacing(12)], [
      el([Font.size(12), Font.color(color_rgb(205, 214, 229))], text("SVG scaling")),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text(
          "The same icon files stay crisp across compact forecast markers and larger showcase sizes."
        )
      ),
      centered_wrapped_cards(cards, 960)
    ])
  end

  defp svg_weather_scale_card(label, note, source) do
    sizes = [24, 48, 80]

    el(
      [
        width(px(300)),
        padding(12),
        spacing(10),
        Background.color(color_rgb(50, 50, 74)),
        Border.rounded(12)
      ],
      column([spacing(10)], [
        row([width(fill()), spacing(8)], [
          el([width(fill()), Font.size(12), Font.color(:white)], text(label)),
          weather_badge("SVG", color_rgba(66, 89, 122, 170 / 255))
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(note)),
        row(
          [width(fill()), spacing(8)],
          Enum.map(sizes, fn size ->
            el(
              [
                width(px(86)),
                height(px(118)),
                padding(8),
                Background.color(color_rgb(34, 34, 50)),
                Border.rounded(10)
              ],
              column([center_x(), center_y(), spacing(8)], [
                svg([width(px(size)), height(px(size)), image_fit(:contain)], source),
                el(
                  [Font.size(10), Font.color(color_rgb(213, 219, 234))],
                  text("#{size}px")
                )
              ])
            )
          end)
        )
      ])
    )
  end

  defp svg_tint_showcase(source, multicolor_source) do
    cards = [
      {"Original", "svg/2 keeps the source stroke color when no tint is set.", nil, "default"},
      {"White tint", "Template tint turns every visible pixel white.", :white,
       "Svg.color(:white)"},
      {"Cyan tint", "Same icon, now themed for cool accents and status states.",
       color_rgb(110, 198, 255), "Svg.color(cyan)"},
      {"Amber tint", "Warm tint for highlights, alerts, and seasonal accents.",
       color_rgb(255, 209, 102), "Svg.color(amber)"}
    ]

    column([spacing(12)], [
      section_title("SVG tint"),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text(
          "svg/2 preserves original colors by default. Svg.color/1 applies template tint to all visible pixels while keeping alpha and edge smoothing."
        )
      ),
      centered_wrapped_cards(
        Enum.map(cards, fn {label, note, tint, tint_label} ->
          svg_tint_card(source, label, note, tint, tint_label)
        end),
        960
      ),
      el(
        [Font.size(11), Font.color(@dim_text)],
        text(
          "Tint also overrides multicolor SVGs, so illustrations and logos flatten into one themed silhouette when Svg.color/1 is set."
        )
      ),
      centered_wrapped_cards(
        [
          svg_tint_card(
            multicolor_source,
            "Multicolor original",
            "The source keeps its four quadrant colors when rendered without tint.",
            nil,
            "tile_quad.svg"
          ),
          svg_tint_card(
            multicolor_source,
            "Multicolor tinted",
            "A single tint overrides all visible colors while preserving the alpha edges.",
            color_rgb(110, 198, 255),
            "Svg.color(cyan)"
          )
        ],
        960
      )
    ])
  end

  defp svg_tint_card(source, label, note, tint, tint_label) do
    cyan_tint = color_rgb(110, 198, 255)
    amber_tint = color_rgb(255, 209, 102)

    svg_attrs =
      [width(px(72)), height(px(72)), image_fit(:contain)] ++
        if(tint, do: [Svg.color(tint)], else: [])

    badge_tone =
      case tint do
        nil -> color_rgba(80, 98, 122, 150 / 255)
        :white -> color_rgba(110, 116, 132, 180 / 255)
        ^cyan_tint -> color_rgba(52, 124, 170, 185 / 255)
        ^amber_tint -> color_rgba(138, 96, 28, 190 / 255)
      end

    el(
      [
        width(px(228)),
        padding(12),
        spacing(10),
        Background.color(color_rgb(46, 48, 72)),
        Border.rounded(12)
      ],
      column([spacing(10)], [
        row([width(fill()), spacing(8)], [
          el([width(fill()), Font.size(12), Font.color(:white)], text(label)),
          weather_badge("svg/2", badge_tone)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(note)),
        el(
          [
            center_x(),
            width(px(132)),
            height(px(120)),
            Background.color(color_rgb(28, 31, 46)),
            Border.width(1),
            Border.color(color_rgba(214, 220, 236, 90 / 255)),
            Border.rounded(12)
          ],
          el([center_x(), center_y()], svg(svg_attrs, source))
        ),
        el(
          [Font.size(10), Font.color(color_rgb(213, 219, 234))],
          text(tint_label)
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
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8),
        Nearby.in_front(asset_preview_mode_badge(mode_label))
      ],
      image([width(fill()), height(fill()), image_fit(fit)], source)
    )
  end

  defp asset_behavior_preview({:svg, source, fit, mode_label}) do
    el(
      [
        width(fill()),
        height(fill()),
        Border.width(1),
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8),
        Nearby.in_front(asset_preview_mode_badge(mode_label))
      ],
      svg([width(fill()), height(fill()), image_fit(fit)], source)
    )
  end

  defp asset_behavior_preview({:background, bg_attr, mode_label}) do
    el(
      [
        width(fill()),
        height(fill()),
        bg_attr,
        Border.width(1),
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8),
        Nearby.in_front(asset_preview_mode_badge(mode_label))
      ],
      none()
    )
  end

  defp asset_preview_mode_badge(label) do
    el(
      [
        align_right(),
        align_bottom(),
        Transform.move_x(-6),
        Transform.move_y(-6),
        padding_each(2, 6, 2, 6),
        Background.color(color_rgba(0, 0, 0, 165 / 255)),
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
        :source -> color_rgb(58, 98, 158)
        :runtime -> color_rgb(48, 120, 102)
        :blocked -> color_rgb(150, 77, 83)
        :background -> color_rgb(92, 80, 164)
        :helper -> color_rgb(88, 92, 124)
        :font_builtin -> color_rgb(84, 106, 94)
        :font -> color_rgb(132, 86, 54)
        :synthetic -> color_rgb(118, 74, 120)
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
    card_w = Kernel.max(stage_w + 20, 220)

    el(
      [
        width(px(card_w)),
        padding(10),
        spacing(8),
        Background.color(color_rgb(45, 45, 68)),
        Border.rounded(10)
      ],
      column([spacing(8)], [
        row([width(fill()), spacing(8)], [
          el([Font.size(11), Font.color(:white)], text(api_label)),
          fit_chip(fit)
        ]),
        el([Font.size(10), Font.color(@dim_text)], text(frame_label)),
        el(
          [Font.size(10), Font.color(color_rgb(184, 188, 210))],
          text("#{frame_w}x#{frame_h}")
        ),
        el(
          [
            center_x(),
            width(px(stage_w)),
            height(px(stage_h)),
            Background.color(color_rgb(31, 31, 45)),
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
        Background.color(color_rgb(24, 24, 36)),
        Border.width(1),
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8)
      ],
      image([width(fill()), height(fill()), image_fit(fit)], source)
    )
  end

  defp fit_demo_preview(:svg, source, fit, {frame_w, frame_h}) do
    el(
      [
        center_x(),
        center_y(),
        width(px(frame_w)),
        height(px(frame_h)),
        Background.color(color_rgb(24, 24, 36)),
        Border.width(1),
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8)
      ],
      svg([width(fill()), height(fill()), image_fit(fit)], source)
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
        Border.color(color_rgba(214, 220, 236, 220 / 255)),
        Border.rounded(8)
      ],
      el(
        [
          center_x(),
          center_y(),
          padding(5),
          Background.color(color_rgba(0, 0, 0, 160 / 255)),
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
        Background.color(color_rgb(52, 110, 124)),
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
        Background.color(color_rgb(142, 84, 52)),
        Border.rounded(6),
        Font.size(10),
        Font.color(:white)
      ],
      text("cover")
    )
  end

  defp fit_legend() do
    column([spacing(4)], [
      el([Font.size(11), Font.color(color_rgb(200, 210, 222))], text("Fit legend")),
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

    bg = if active, do: @blue, else: color_rgb(45, 45, 65)
    pressed_bg = if active, do: color_rgb(55, 82, 206), else: color_rgb(40, 42, 60)
    text_color = if active, do: @light_text, else: @dim_text

    border_color =
      if active, do: color_rgb(126, 148, 230), else: color_rgb(86, 92, 122)

    hover_attrs =
      if active do
        [
          Interactive.mouse_over([
            Background.color(color_rgb(74, 108, 240)),
            Font.color(@light_text),
            Border.color(color_rgb(152, 174, 246))
          ])
        ]
      else
        [
          Interactive.mouse_over([
            Background.color(color_rgb(70, 70, 100)),
            Font.color(@light_text),
            Border.color(color_rgb(132, 142, 186))
          ])
        ]
      end

    Emerge.UI.Input.button(
      [
        key({:menu, page}),
        width(fill()),
        padding_xy(10, 8),
        Background.color(bg),
        Border.rounded(10),
        Border.width(1),
        Border.color(border_color),
        Font.size(12),
        Font.color(text_color),
        Event.on_press({self(), {:demo_nav, page}}),
        Interactive.focused([
          Border.color(color_rgb(166, 186, 236)),
          Border.glow(color_rgba(132, 158, 232, 110 / 255), 2)
        ]),
        Interactive.mouse_down([
          Background.color(pressed_bg),
          Border.color(color_rgb(176, 190, 228)),
          Border.inner_shadow(
            offset: {0, 1},
            blur: 6,
            size: 1,
            color: color_rgba(0, 0, 0, 120 / 255)
          ),
          Transform.move_y(1)
        ])
      ] ++ hover_attrs,
      text(label)
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
          case Emerge.Engine.lookup_event(state, id_bin, event_type) do
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
              Emerge.Engine.dispatch_event(state, id_bin, event_type)

              {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
          end

        {id_bin, event_type, route}
        when is_binary(id_bin) and event_type in [:key_down, :key_up, :key_press] and
               is_binary(route) ->
          event_ref = {event_type, route}

          case Emerge.Engine.lookup_event(state, id_bin, event_ref) do
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
              Emerge.Engine.dispatch_event(state, id_bin, event_ref)

              {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
          end

        {id_bin, event_type, payload} when is_binary(id_bin) and is_atom(event_type) ->
          case Emerge.Engine.lookup_event(state, id_bin, event_type) do
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
              Emerge.Engine.dispatch_event(state, id_bin, event_type, payload)

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
        {_, :key_down, _} -> event_log
        {_, :key_up, _} -> event_log
        {_, :key_press, _} -> event_log
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
         {:demo_event, :animation_shelf, :toggle},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:animation_shelf_open, false)
    open? = !previous
    Process.put(:animation_shelf_open, open?)

    entry = if open?, do: "Shelf: opened", else: "Shelf: closed"
    new_log = Enum.take([entry | event_log], 20)
    changed = previous != open? or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
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
         {:demo_event, :soft_keyboard, :toggle_shift},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_soft_shift, false)
    popup = Process.get(:demo_soft_popup, nil)
    shift_active = !previous

    Process.put(:demo_soft_shift, shift_active)
    Process.put(:demo_soft_popup, nil)

    entry = if shift_active, do: "Soft keyboard: shift on", else: "Soft keyboard: shift off"
    Process.put(:demo_soft_last_action, entry)

    new_log = Enum.take([entry | event_log], 20)
    changed = previous != shift_active or popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :soft_keyboard, {:show_alternates, key}},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       )
       when key in [:a, :e] do
    previous_popup = Process.get(:demo_soft_popup, nil)
    hold_count = Process.get(:demo_soft_hold_count, 0) + 1

    Process.put(:demo_soft_popup, key)
    Process.put(:demo_soft_hold_count, hold_count)

    entry =
      "Soft keyboard hold: #{String.upcase(Atom.to_string(key))} alternates (#{hold_count})"

    Process.put(:demo_soft_last_action, entry)

    new_log = Enum.take([entry | event_log], 20)
    changed = previous_popup != key or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :input_changed, value},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       )
       when is_binary(value) do
    previous = Process.get(:demo_input_value, "")
    previous_popup = Process.get(:demo_soft_popup, nil)
    Process.put(:demo_input_value, value)
    Process.put(:demo_input_preedit, nil)
    Process.put(:demo_input_preedit_cursor, nil)
    Process.put(:demo_soft_popup, nil)

    if previous_popup != nil do
      Process.put(:demo_soft_last_action, "Soft keyboard: alternate committed")
    end

    changed_value = value != previous

    new_log =
      if changed_value do
        preview =
          if String.length(value) > 42, do: String.slice(value, 0, 39) <> "...", else: value

        Enum.take(["Input change: #{preview}" | event_log], 20)
      else
        event_log
      end

    changed = changed_value or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :input_focus},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_input_focused, false)
    count = Process.get(:demo_input_focus_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_input_focused, true)
    Process.put(:demo_input_focus_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Input focus (#{count})" | event_log], 20)
    changed = !previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :input_blur},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_input_focused, false)
    count = Process.get(:demo_input_blur_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_input_focused, false)
    Process.put(:demo_input_blur_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Input blur (#{count})" | event_log], 20)
    changed = previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :button_press},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    count = Process.get(:demo_button_press_count, 0) + 1
    Process.put(:demo_button_press_count, count)

    new_log = Enum.take(["Button press (#{count})" | event_log], 20)
    changed = new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :button_focus},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_button_focused, false)
    count = Process.get(:demo_button_focus_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_button_focused, true)
    Process.put(:demo_button_focus_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Button focus (#{count})" | event_log], 20)
    changed = !previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :button_blur},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_button_focused, false)
    count = Process.get(:demo_button_blur_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_button_focused, false)
    Process.put(:demo_button_blur_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Button blur (#{count})" | event_log], 20)
    changed = previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :keyboard_listener, :focus},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_key_listener_focused, false)
    count = Process.get(:demo_key_listener_focus_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_key_listener_focused, true)
    Process.put(:demo_key_listener_focus_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Keyboard pad focus (#{count})" | event_log], 20)
    changed = !previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :keyboard_listener, :blur},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    previous = Process.get(:demo_key_listener_focused, false)
    count = Process.get(:demo_key_listener_blur_count, 0) + 1
    previous_popup = Process.get(:demo_soft_popup, nil)

    Process.put(:demo_key_listener_focused, false)
    Process.put(:demo_key_listener_blur_count, count)
    Process.put(:demo_soft_popup, nil)

    new_log = Enum.take(["Keyboard pad blur (#{count})" | event_log], 20)
    changed = previous or previous_popup != nil or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :keyboard_listener, {:key_down, binding}},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    count = Process.get(:demo_key_listener_key_down_count, 0) + 1
    Process.put(:demo_key_listener_key_down_count, count)

    {entry, changed_binding} =
      case binding do
        :enter ->
          next = Process.get(:demo_key_listener_enter_count, 0) + 1
          Process.put(:demo_key_listener_enter_count, next)
          {"Keyboard key down: Enter (#{next})", true}

        :ctrl_digit_1 ->
          next = Process.get(:demo_key_listener_ctrl_digit_count, 0) + 1
          Process.put(:demo_key_listener_ctrl_digit_count, next)
          {"Keyboard key down: Ctrl+1 (#{next})", true}

        :arrow_left ->
          next = Process.get(:demo_key_listener_arrow_left_count, 0) + 1
          Process.put(:demo_key_listener_arrow_left_count, next)
          {"Keyboard key down: Arrow Left (#{next})", true}

        other ->
          {"Keyboard key down: #{inspect(other)}", false}
      end

    Process.put(:demo_key_listener_last_action, entry)

    new_log = Enum.take([entry | event_log], 20)

    changed = changed_binding or new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :keyboard_listener, {:key_up, binding}},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    count = Process.get(:demo_key_listener_key_up_count, 0) + 1
    Process.put(:demo_key_listener_key_up_count, count)

    entry =
      case binding do
        :escape ->
          next = Process.get(:demo_key_listener_escape_count, 0) + 1
          Process.put(:demo_key_listener_escape_count, next)
          "Keyboard key up: Escape (#{next})"

        other ->
          "Keyboard key up: #{inspect(other)}"
      end

    Process.put(:demo_key_listener_last_action, entry)

    new_log = Enum.take([entry | event_log], 20)
    changed = new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :keyboard_listener, {:key_press, binding}},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       ) do
    count = Process.get(:demo_key_listener_key_press_count, 0) + 1
    Process.put(:demo_key_listener_key_press_count, count)

    entry =
      case binding do
        :space ->
          next = Process.get(:demo_key_listener_space_press_count, 0) + 1
          Process.put(:demo_key_listener_space_press_count, next)
          "Keyboard key press: Space (#{next})"

        other ->
          "Keyboard key press: #{inspect(other)}"
      end

    Process.put(:demo_key_listener_last_action, entry)

    new_log = Enum.take([entry | event_log], 20)
    changed = new_log != event_log

    {{mouse_pos, new_log, size, scale, current_page, last_move_label, unstable_items}, changed}
  end

  defp process_event(
         {:demo_event, :swipe_showcase, direction},
         _state,
         {mouse_pos, event_log, size, scale, current_page, last_move_label, unstable_items}
       )
       when direction in [:up, :down, :left, :right] do
    {count_key, label} =
      case direction do
        :up -> {:demo_swipe_up_count, "Up"}
        :down -> {:demo_swipe_down_count, "Down"}
        :left -> {:demo_swipe_left_count, "Left"}
        :right -> {:demo_swipe_right_count, "Right"}
      end

    count = Process.get(count_key, 0) + 1
    previous_last = Process.get(:demo_swipe_last_direction, "None")

    Process.put(count_key, count)
    Process.put(:demo_swipe_last_direction, label)

    entry = "Swipe #{label} (#{count})"
    new_log = Enum.take([entry | event_log], 20)
    changed = new_log != event_log or previous_last != label

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
    count = Kernel.min(count, length(children))
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
        Event.on_click({self(), {:feature_click, title}}),
        spacing(8),
        padding(15),
        Background.color(bg_color),
        Border.rounded(8)
      ],
      [
        el([Font.size(16), Font.color(:white)], text(title)),
        el([Font.size(12), Font.color(color_rgb(200, 200, 220))], text(description))
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
          [Font.size(11), Font.color(color_rgb(210, 210, 230))],
          text("Triggers #{format_event_label(event)}")
        )
      ])
    )
  end

  defp transformed_event_showcase(
         label,
         transform_note,
         instruction,
         transform_attrs,
         state_attrs,
         events,
         bg_color
       ) do
    column(
      [width(px(238)), spacing(8)],
      [
        el([Font.size(13), Font.color(@light_text)], text(label)),
        el([Font.size(11), Font.color(@dim_text)], text(instruction)),
        el(
          [
            width(fill()),
            padding(12),
            Background.color(color_rgb(44, 44, 66)),
            Border.rounded(10)
          ],
          column([spacing(8)], [
            el(
              [
                width(fill()),
                height(px(150)),
                Background.color(color_rgba(255, 255, 255, 12 / 255)),
                Border.rounded(12)
              ],
              el(
                [
                  width(px(128)),
                  height(px(82)),
                  center_x(),
                  center_y(),
                  Background.color(color_rgba(255, 255, 255, 8 / 255)),
                  Border.width(1),
                  Border.color(color_rgba(208, 216, 240, 110 / 255)),
                  Border.rounded(12),
                  Nearby.in_front(
                    transformed_event_target(
                      label,
                      transform_note,
                      transform_attrs,
                      state_attrs,
                      events,
                      bg_color
                    )
                  )
                ],
                el(
                  [
                    center_x(),
                    align_bottom(),
                    Transform.move_y(-8),
                    Font.size(9),
                    Font.color(color_rgba(215, 222, 242, 170 / 255))
                  ],
                  text("Original slot")
                )
              )
            ),
            el(
              [Font.size(10), Font.color(color_rgb(204, 214, 236))],
              text(transform_note)
            )
          ])
        )
      ]
    )
  end

  defp transformed_event_target(
         label,
         transform_note,
         transform_attrs,
         state_attrs,
         events,
         bg_color
       ) do
    el(
      [
        width(px(128)),
        height(px(82)),
        center_x(),
        center_y(),
        padding(12),
        Background.color(bg_color),
        Border.width(1),
        Border.color(color_rgba(245, 248, 255, 120 / 255)),
        Border.rounded(12)
      ] ++ transform_attrs ++ state_attrs ++ event_attrs(events, label),
      column([center_x(), center_y(), spacing(5)], [
        el([Font.size(13), Font.color(:white)], text(label)),
        el(
          [Font.size(10), Font.color(color_rgba(245, 248, 255, 215 / 255))],
          text(transform_note)
        )
      ])
    )
  end

  defp enter_shelf_showcase() do
    shelf_open = Process.get(:animation_shelf_open, false)
    toggle_label = if shelf_open, do: "Close shelf", else: "Open shelf"
    status_label = if shelf_open, do: "Close -> exit", else: "Open -> enter"

    column([width(fill()), spacing(8)], [
      el([Font.size(13), Font.color(@light_text)], text("Click-inserted side shelf")),
      el(
        [
          width(fill()),
          padding(12),
          Background.color(color_rgb(44, 44, 66)),
          Border.rounded(10)
        ],
        column([spacing(10)], [
          row([width(fill()), spacing(10), align_bottom()], [
            el(
              [width(fill()), Font.size(11), Font.color(@dim_text)],
              text(
                "Open mounts the live shelf with animate_enter. Close removes it from events immediately, then animate_exit finishes the passive ghost."
              )
            ),
            el(
              [
                padding_xy(8, 6),
                Background.color(color_rgb(56, 60, 86)),
                Border.rounded(999),
                Font.size(10),
                Font.color(color_rgb(216, 222, 242))
              ],
              text(status_label)
            ),
            Emerge.UI.Input.button(
              [
                padding_xy(12, 8),
                Font.size(12),
                Font.color(color_rgb(238, 242, 252)),
                Background.color(color_rgb(66, 84, 146)),
                Border.rounded(9),
                Border.width(1),
                Border.color(color_rgb(150, 176, 244)),
                Event.on_press({self(), {:demo_event, :animation_shelf, :toggle}}),
                Interactive.mouse_over([
                  Background.color(color_rgb(78, 98, 168)),
                  Border.color(color_rgb(176, 198, 252))
                ]),
                Interactive.mouse_down([
                  Background.color(color_rgb(58, 74, 128)),
                  Border.color(color_rgb(184, 198, 236)),
                  Transform.move_y(1)
                ])
              ],
              text(toggle_label)
            )
          ]),
          el(
            [
              width(fill()),
              height(px(244)),
              padding(14),
              Background.color(color_rgb(32, 35, 52)),
              Border.rounded(14),
              Border.width(1),
              Border.color(color_rgb(86, 96, 132))
            ],
            row(
              [width(fill()), height(fill()), spacing(12), align_top()],
              [animation_shelf_workspace()] ++ animation_shelf_panel(shelf_open)
            )
          ),
          el(
            [Font.size(10), Font.color(color_rgb(204, 214, 236))],
            text(
              "Animation.animate_enter(width + alpha + move_x, 260ms, :ease_out) + Animation.animate_exit(width + alpha + move_x, 220ms, :ease_in)"
            )
          )
        ])
      )
    ])
  end

  defp animation_shelf_workspace() do
    el(
      [
        key(:animation_shelf_workspace),
        width(fill()),
        height(fill()),
        padding(16),
        spacing(12),
        Background.color(color_rgb(48, 53, 76)),
        Border.rounded(12)
      ],
      column([spacing(10)], [
        el([Font.size(15), Font.color(:white)], text("Workbench")),
        el(
          [Font.size(11), Font.color(color_rgb(210, 216, 236))],
          text("The main surface stays mounted. Only the shelf is inserted and removed.")
        ),
        row([width(fill()), spacing(8)], [
          info_pill("Host stays stable", color_rgb(82, 102, 156)),
          info_pill("Shelf remounts", color_rgb(96, 88, 142))
        ]),
        el(
          [
            width(fill()),
            height(fill()),
            padding(12),
            Background.color(color_rgba(255, 255, 255, 14 / 255)),
            Border.rounded(10),
            Border.width(1),
            Border.color(color_rgba(216, 224, 246, 70 / 255))
          ],
          column([spacing(8)], [
            el([Font.size(12), Font.color(color_rgb(232, 236, 248))], text("Canvas area")),
            el(
              [Font.size(10), Font.color(color_rgb(194, 204, 228))],
              text("This row layout widens when the side shelf is mounted.")
            )
          ])
        )
      ])
    )
  end

  defp animation_shelf_panel(true) do
    [
      column(
        [
          key(:animation_shelf_panel),
          width(px(176)),
          height(fill()),
          padding(14),
          spacing(10),
          Transform.alpha(1.0),
          Transform.move_x(0),
          Background.color(color_rgb(86, 66, 124)),
          Border.rounded(12),
          Border.width(1),
          Border.color(color_rgb(196, 176, 236)),
          Animation.animate_enter(
            [
              [width(px(28)), Transform.alpha(0.0), Transform.move_x(16)],
              [width(px(176)), Transform.alpha(1.0), Transform.move_x(0)]
            ],
            260,
            :ease_out
          ),
          Animation.animate_exit(
            [
              [width(px(176)), Transform.alpha(1.0), Transform.move_x(0)],
              [width(px(32)), Transform.alpha(0.0), Transform.move_x(18)]
            ],
            220,
            :ease_in
          )
        ],
        [
          el([Font.size(14), Font.color(:white)], text("Shelf")),
          el(
            [Font.size(10), Font.color(color_rgb(234, 226, 248))],
            text("Enter on mount, exit on close")
          ),
          info_pill("Activity", color_rgb(112, 90, 154)),
          info_pill("Filters", color_rgb(98, 80, 144)),
          info_pill("Notes", color_rgb(86, 72, 136))
        ]
      )
    ]
  end

  defp animation_shelf_panel(false), do: []

  defp info_pill(label, bg_color) do
    el(
      [
        width(fill()),
        padding_xy(10, 8),
        Background.color(bg_color),
        Border.rounded(999),
        Font.size(11),
        Font.color(:white)
      ],
      text(label)
    )
  end

  defp event_attrs(events, label) do
    Enum.map(events, &event_attr(&1, label))
  end

  defp event_attr(:mouse_down, label),
    do: Event.on_mouse_down({self(), {:demo_event, label, :mouse_down}})

  defp event_attr(:mouse_up, label),
    do: Event.on_mouse_up({self(), {:demo_event, label, :mouse_up}})

  defp event_attr(:mouse_enter, label),
    do: Event.on_mouse_enter({self(), {:demo_event, label, :mouse_enter}})

  defp event_attr(:mouse_leave, label),
    do: Event.on_mouse_leave({self(), {:demo_event, label, :mouse_leave}})

  defp event_attr(:mouse_move, label),
    do: Event.on_mouse_move({self(), {:demo_event, label, :mouse_move}})

  defp format_event_label(event) do
    event
    |> Atom.to_string()
    |> String.replace("_", " ")
  end

  defp chip(label) do
    el(
      [
        padding(6),
        Background.color(color_rgb(55, 60, 90)),
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
