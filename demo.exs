# Demo script for EmergeSkia
# Run with: mix run demo.exs

IO.puts("Starting EmergeSkia demo...")

{:ok, renderer} = EmergeSkia.start("EmergeSkia Demo", 800, 600)

EmergeSkia.set_input_target(renderer, self())

defmodule Demo do
  import Emerge.UI

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

  def format_event(event) do
    inspect(event)
  end

  def build_tree({_width, _height}, {mx, my}, event_log) do
    column([width(:fill), height(:fill), padding(20), spacing(20), Emerge.UI.Background.color(@dark_bg)], [
      el(
        [padding(16), Emerge.UI.Background.gradient(@blue, @purple, 90), Emerge.UI.Border.rounded(12)],
        el([Emerge.UI.Font.size(26), Emerge.UI.Font.color(@light_text)], text("EmergeSkia Demo"))
      ),
      row([spacing(20), width(:fill)], [
        el(
          [width(:fill), padding(16), Emerge.UI.Background.color(@event_bg), Emerge.UI.Border.rounded(12)],
          column([spacing(6)], [
            el([Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)], text("Direct Skia Rendering")),
            el([Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)], text("No Scenic overhead")),
            el([Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)], text("Minimal NIF surface")),
            el([Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)], text("Rust layout + renderer"))
          ])
        ),
        el(
          [width(:fill), padding(16), Emerge.UI.Background.color(@event_bg), Emerge.UI.Border.rounded(12)],
          column([spacing(6)], [
            el([Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)], text("Mouse Position")),
            el([Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)], text("X: #{Float.round(mx, 1)}")),
            el([Emerge.UI.Font.size(14), Emerge.UI.Font.color(@dim_text)], text("Y: #{Float.round(my, 1)}"))
          ])
        )
      ]),
      el(
        [
          width(:fill),
          height(:fill),
          padding(16),
          Emerge.UI.Background.color(@event_bg),
          Emerge.UI.Border.rounded(12)
        ],
        column([spacing(8)], [
          el([Emerge.UI.Font.size(18), Emerge.UI.Font.color(@light_text)], text("Input Event Log")),
          column(
            [spacing(4)],
            event_log
            |> Enum.take(8)
            |> Enum.map(fn line ->
              el([Emerge.UI.Font.size(13), Emerge.UI.Font.color(@dim_text)], text(line))
            end)
          )
        ])
      ),
      el(
        [padding(6), Emerge.UI.Background.color(@pink), Emerge.UI.Border.rounded(6)],
        el(
          [Emerge.UI.Font.size(12), Emerge.UI.Font.color(@light_text)],
          text("Cursor: #{Float.round(mx, 1)}, #{Float.round(my, 1)}")
        )
      )
    ])
  end

  def drain_mailbox(acc \\ []) do
    receive do
      {:emerge_skia_event, event} -> drain_mailbox([event | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  def process_events(events, mouse_pos, event_log, size, scale) do
    Enum.reduce(events, {mouse_pos, event_log, size, scale}, fn event, {pos, log, sz, sc} ->
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
            process_events(all_events, mouse_pos, event_log, size, scale)

          tree = build_tree(new_size, new_mouse_pos, new_log)

          {patch_bin, next_state, _assigned} = Emerge.diff_state_update(state, tree)
          case EmergeSkia.Native.renderer_patch(renderer, patch_bin, elem(new_size, 0), elem(new_size, 1), new_scale) do
            :ok -> :ok
            {:ok, _} -> :ok
            {:error, reason} -> raise "renderer_patch failed: #{reason}"
          end

          run_loop(renderer, next_state, new_mouse_pos, new_log, new_size, new_scale)
      after
        100 ->
          run_loop(renderer, state, mouse_pos, event_log, size, scale)
      end
    end
  end

end

IO.puts("Window opened! Move mouse, click, press keys. Close window to exit.")

initial_size = {800.0, 600.0}
initial_scale = 1.0
initial_tree = Demo.build_tree(initial_size, {400.0, 300.0}, [])

state = Emerge.diff_state_new()
{full_bin, state, _assigned} = Emerge.encode_full(state, initial_tree)
case EmergeSkia.Native.renderer_upload(renderer, full_bin, elem(initial_size, 0), elem(initial_size, 1), initial_scale) do
  :ok -> :ok
  {:ok, _} -> :ok
  {:error, reason} -> raise "renderer_upload failed: #{reason}"
end

Demo.run_loop(renderer, state, {400.0, 300.0}, [], initial_size, initial_scale)

IO.puts("Window closed. Demo complete!")
