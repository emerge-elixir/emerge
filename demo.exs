# Demo script for EmergeSkia
# Run with: mix run demo.exs

IO.puts("Starting EmergeSkia demo...")

{:ok, renderer} = EmergeSkia.start("EmergeSkia Demo", 800, 600)

# Set up input handling - events will be sent directly to this process
EmergeSkia.set_input_target(renderer, self())

defmodule Demo do
  @dark_bg 0x1a1a2eFF
  @blue 0x4361eeFF
  @purple 0x7209b7FF
  @pink 0xf72585FF
  @light_text 0xffffffFF
  @dim_text 0xaaaaaaFF
  @event_bg 0x2d2d44FF

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

  def render(renderer, mouse_pos, event_log) do
    {mx, my} = mouse_pos

    # Static UI elements
    static_commands = [
      # Background
      {:rect, 0.0, 0.0, 800.0, 600.0, @dark_bg},

      # Header bar with gradient
      {:gradient, 0.0, 0.0, 800.0, 80.0, @blue, @purple, 90.0},

      # Title text
      {:text, 30.0, 50.0, "EmergeSkia Demo", 28.0, @light_text},

      # Card 1
      {:rounded_rect, 30.0, 120.0, 350.0, 180.0, 12.0, @event_bg},
      {:text, 50.0, 160.0, "Direct Skia Rendering", 18.0, @light_text},
      {:text, 50.0, 195.0, "No Scenic overhead", 14.0, @dim_text},
      {:text, 50.0, 220.0, "Minimal NIF interface", 14.0, @dim_text},
      {:text, 50.0, 245.0, "Push-based input events", 14.0, @dim_text},

      # Card 2 - Mouse position display
      {:rounded_rect, 420.0, 120.0, 350.0, 180.0, 12.0, @event_bg},
      {:text, 440.0, 160.0, "Mouse Position", 18.0, @light_text},
      {:text, 440.0, 200.0, "X: #{Float.round(mx, 1)}", 16.0, @dim_text},
      {:text, 440.0, 230.0, "Y: #{Float.round(my, 1)}", 16.0, @dim_text},

      # Event log area
      {:rounded_rect, 30.0, 320.0, 740.0, 250.0, 12.0, @event_bg},
      {:text, 50.0, 355.0, "Input Event Log", 18.0, @light_text},
      {:border, 30.0, 320.0, 740.0, 250.0, 12.0, 2.0, 0x4361ee80},

      # Mouse cursor indicator (small circle at cursor position)
      {:rounded_rect, mx - 5, my - 5, 10.0, 10.0, 5.0, @pink}
    ]

    # Event log entries (show last 8 events)
    event_commands =
      event_log
      |> Enum.take(8)
      |> Enum.with_index()
      |> Enum.map(fn {event_str, idx} ->
        {:text, 50.0, 385.0 + idx * 22.0, event_str, 14.0, @dim_text}
      end)

    EmergeSkia.render(renderer, static_commands ++ event_commands)
  end

  # Drain all pending input events from the mailbox
  def drain_mailbox(acc \\ []) do
    receive do
      {:emerge_skia_event, event} -> drain_mailbox([event | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end

  # Process a batch of events, returning {new_mouse_pos, new_event_log}
  def process_events(events, mouse_pos, event_log) do
    Enum.reduce(events, {mouse_pos, event_log}, fn event, {pos, log} ->
      # Update mouse position if it's a cursor event
      new_pos =
        case event do
          {:cursor_pos, {x, y}} -> {x, y}
          {:cursor_button, {_, _, _, {x, y}}} -> {x, y}
          _ -> pos
        end

      # Add formatted event to log (skip cursor_pos to reduce noise)
      new_log =
        case event do
          {:cursor_pos, _} -> log
          _ -> [format_event(event) | log]
        end

      {new_pos, Enum.take(new_log, 20)}
    end)
  end

  def run_loop(renderer, mouse_pos, event_log) do
    if EmergeSkia.running?(renderer) do
      # Wait for at least one event
      receive do
        {:emerge_skia_event, first_event} ->
          # Drain all remaining events from mailbox
          remaining = drain_mailbox()
          all_events = [first_event | remaining]

          # Process all events at once
          {new_mouse_pos, new_log} = process_events(all_events, mouse_pos, event_log)

          # Re-render with updated state
          render(renderer, new_mouse_pos, new_log)
          run_loop(renderer, new_mouse_pos, new_log)
      after
        100 ->
          # Keep loop alive
          run_loop(renderer, mouse_pos, event_log)
      end
    end
  end
end

IO.puts("Window opened! Move mouse, click, press keys. Close window to exit.")

# Initial render
Demo.render(renderer, {400.0, 300.0}, [])

# Run the event loop
Demo.run_loop(renderer, {400.0, 300.0}, [])

IO.puts("Window closed. Demo complete!")
