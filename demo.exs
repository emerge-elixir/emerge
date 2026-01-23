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

  def build_tree({width, height}, {mx, my}, event_log) do
    column([width(:fill), height(:fill), padding(20), spacing(20), Background.color(@dark_bg)], [
      el([padding(16), Background.gradient(@blue, @purple, 90), Border.rounded(12)],
        el([Font.size(26), Font.color(@light_text)], text("EmergeSkia Demo"))
      ),
      row([spacing(20), width(:fill)], [
        el([width(:fill), padding(16), Background.color(@event_bg), Border.rounded(12)],
          column([spacing(6)], [
            el([Font.size(18), Font.color(@light_text)], text("Direct Skia Rendering")),
            el([Font.size(14), Font.color(@dim_text)], text("No Scenic overhead")),
            el([Font.size(14), Font.color(@dim_text)], text("Minimal NIF surface")),
            el([Font.size(14), Font.color(@dim_text)], text("Rust layout + renderer"))
          ])
        ),
        el([width(:fill), padding(16), Background.color(@event_bg), Border.rounded(12)],
          column([spacing(6)], [
            el([Font.size(18), Font.color(@light_text)], text("Mouse Position")),
            el([Font.size(14), Font.color(@dim_text)], text("X: #{Float.round(mx, 1)}")),
            el([Font.size(14), Font.color(@dim_text)], text("Y: #{Float.round(my, 1)}"))
          ])
        )
      ]),
      el([width(:fill), height(:fill), padding(16), Background.color(@event_bg), Border.rounded(12)],
        column([spacing(8)], [
          el([Font.size(18), Font.color(@light_text)], text("Input Event Log")),
          column(
            [spacing(4)],
            event_log
            |> Enum.take(8)
            |> Enum.map(fn line ->
              el([Font.size(13), Font.color(@dim_text)], text(line))
            end)
          )
        ])
      ),
      el([padding(6), Background.color(@pink), Border.rounded(6)],
        el([Font.size(12), Font.color(@light_text)],
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

  def process_events(events, mouse_pos, event_log, size) do
    Enum.reduce(events, {mouse_pos, event_log, size}, fn event, {pos, log, size} ->
      new_pos =
        case event do
          {:cursor_pos, {x, y}} -> {x, y}
          {:cursor_button, {_, _, _, {x, y}}} -> {x, y}
          _ -> pos
        end

      new_size =
        case event do
          {:resized, {w, h, _scale}} -> {w, h}
          _ -> size
        end

      new_log =
        case event do
          {:cursor_pos, _} -> log
          _ -> [format_event(event) | log]
        end

      {new_pos, Enum.take(new_log, 20), new_size}
    end)
  end

  def run_loop(renderer, state, mouse_pos, event_log, size) do
    if EmergeSkia.running?(renderer) do
      receive do
        {:emerge_skia_event, first_event} ->
          remaining = drain_mailbox()
          all_events = [first_event | remaining]

          {new_mouse_pos, new_log, new_size} = process_events(all_events, mouse_pos, event_log, size)
          tree = build_tree(new_size, new_mouse_pos, new_log)
          {state, _assigned} = EmergeSkia.patch_tree(renderer, state, tree, elem(new_size, 0), elem(new_size, 1))

          run_loop(renderer, state, new_mouse_pos, new_log, new_size)
      after
        100 ->
          run_loop(renderer, state, mouse_pos, event_log, size)
      end
    end
  end
end

IO.puts("Window opened! Move mouse, click, press keys. Close window to exit.")

initial_size = {800.0, 600.0}
initial_tree = Demo.build_tree(initial_size, {400.0, 300.0}, [])
{state, _assigned} = EmergeSkia.upload_tree(renderer, initial_tree, elem(initial_size, 0), elem(initial_size, 1))

Demo.run_loop(renderer, state, {400.0, 300.0}, [], initial_size)

IO.puts("Window closed. Demo complete!")
