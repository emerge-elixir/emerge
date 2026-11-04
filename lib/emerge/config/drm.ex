defmodule Emerge.Config.DRM do
  @moduledoc """
  A display connected directly to Linux DRM/KMS, without a window manager.
  Set this record as `Emerge.Config.output`.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{card: "/dev/dri/card0", connector: "DSI-1"},
    renderer: %Emerge.Config.Renderer{api: :opengl}
  }
  ```

  ## Fields

  - `card` — absolute path to the display's KMS device; defaults to
    `"/dev/dri/card0"`. A stable `/dev/dri/by-path/...-card` path is also accepted.
  - `connector` — connector name, such as `"DSI-1"` or `"DP-1"`. The default `nil`
    selects an available output automatically.
  - `mode` — an advertised mode ID or a mode map returned by
    `EmergeSkia.drm_outputs/1`. Defaults to `nil` for automatic mode selection.
    An explicit mode also requires an explicit connector. Invalid selections
    return an error rather than silently choosing another display or mode.
  - `preferred_size` — optional `{width, height}` hint for automatic mode selection.
    Defaults to `nil`. The selected mode determines the actual display size.
  - `display` — `:on` (default) or `:off`. Off blanks the display and ignores its
    UI input; it does not suspend your application or the operating system.
  - `wake_on_touch` — `nil` (default), `:single_tap`, `:double_tap` or
    `:long_press`. `nil` disables touch wake. When enabled, the chosen gesture
    turns an off display on without activating the UI underneath it.
  - `backlight` — `Emerge.Config.Backlight`, defaulting to automatic association.
    Use it to select a panel backlight and request brightness.
  - `hotplug_interval_ms` — connector hotplug polling interval in milliseconds;
    defaults to `750`. Set it to `nil` to disable polling for built-in panels.

  Discover connector names and modes with `EmergeSkia.drm_outputs(drm_card: path)`.

  ## Startup and boot splash

  When starting with `display: :on`, Emerge leaves any image already on the
  display, such as a boot splash, visible while it prepares the first application
  frame. It also leaves the backlight unchanged until that frame is presented.
  No special configuration is needed.

  If the active output and display mode match the selected output and mode,
  Emerge replaces the image without requesting a modeset. A mode change may
  cause a brief blank when the first frame is presented. The result also depends
  on the display hardware and driver; a seamless transition cannot be guaranteed
  on every device.

  Input is enabled and any requested brightness is applied after the first frame
  is presented. Starting with `display: :off`, turning the display off, stopping,
  or cleaning up after a failure can blank the display. Stopping does not restore
  the previous image or backlight level.

  ## Changing settings

  `display`, `wake_on_touch` and backlight brightness can change through `config` with an explicit
  OpenGL, Vulkan or raster renderer. Raster supports CPU presentation (including
  its `:auto` default) and explicit `raster_present: :gpu_upload`, which requires
  the OpenGL DRM presenter build. Automatic API selection does not support these
  controls. Turning the display off destroys the internal presenter. Turning it
  on creates a new one and enables a managed backlight after the first application
  frame is presented. GPU resources are not restored.

  Changing the card, connector, mode, size hint, backlight device or hotplug polling
  interval requires a new viewport.

  ## Waking by touch

  Set `wake_on_touch: :single_tap`, `:double_tap` or `:long_press` to wake an off
  display. The default `nil` disables all touch wake gestures. Raw touch
  subscriptions are not required. The waking contact sequence is consumed, so it
  cannot also activate a UI element. Mouse and keyboard input do not trigger wake.

  ```elixir
  %Emerge.Config.DRM{display: :off, wake_on_touch: :double_tap}
  ```

  A tap must end within 250 ms and move no more than 12 output-coordinate pixels.
  A double tap allows 300 ms between taps and 24 pixels between their starting
  positions. A long press wakes after 600 ms within the same 12-pixel movement
  limit. Multiple simultaneous contacts cancel recognition.

  A wake changes the desired `display` to `:on`. To also receive
  `{:display_wake, gesture}` through `c:Emerge.handle_input/2`, subscribe explicitly:

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{display: :off, wake_on_touch: :double_tap},
    renderer: [api: :opengl],
    input: [events: [:display_wake]]
  }
  ```

  With `input.events: []` or `:touch` alone, waking still works but no wake event
  reaches your handler. If your `config/2` derives display state from application
  state, subscribe and update that state in this handler. A later explicit `:off`
  still wins. Requested brightness is preserved, including zero. This event signals
  accepted wake intent, not physical presentation.

  ```elixir
  def handle_input({:display_wake, _gesture}, state) do
    {:noreply, %{state | sleeping?: false}}
  end

  def handle_input(_event, state), do: {:noreply, state}
  ```

  This does not resume a suspended system: the touchscreen must keep producing
  events while the display is off. Keep another wake source available if the
  driver powers down touch with the panel. Touch sources follow existing DRM
  input routing; a shared touchscreen can wake multiple opted-in viewports.

  Standalone `EmergeSkia` callers must register an input target with
  `EmergeSkia.set_input_target/2` to enable touch wake. That target receives
  `{:emerge_skia_display_wake, {session_id, revision}, gesture}` when the on intent
  is accepted, followed by the ordinary config completion. Keep your stored config
  in sync with that intent. An initially-off standalone renderer still needs its
  first tree upload before it can present a frame. Neither notification guarantees
  physical panel behavior.
  """
  defstruct card: "/dev/dri/card0",
            connector: nil,
            mode: nil,
            preferred_size: nil,
            display: :on,
            wake_on_touch: nil,
            backlight: %Emerge.Config.Backlight{},
            hotplug_interval_ms: 750

  @typedoc "Desired display state, not application or system power state."
  @type display :: :on | :off
  @type wake_gesture :: :single_tap | :double_tap | :long_press
  @type t :: %__MODULE__{
          card: String.t(),
          connector: String.t() | nil,
          mode: String.t() | map() | nil,
          preferred_size: {pos_integer(), pos_integer()} | nil,
          display: display(),
          wake_on_touch: wake_gesture() | nil,
          backlight: Emerge.Config.Backlight.t() | Emerge.Config.Backlight.options(),
          hotplug_interval_ms: pos_integer() | nil
        }
  @typedoc "Canonical form after nested keyword records have been expanded."
  @type normalized_t :: %__MODULE__{
          card: String.t(),
          connector: String.t() | nil,
          mode: String.t() | map() | nil,
          preferred_size: {pos_integer(), pos_integer()} | nil,
          display: display(),
          wake_on_touch: wake_gesture() | nil,
          backlight: Emerge.Config.Backlight.normalized_t(),
          hotplug_interval_ms: pos_integer() | nil
        }

  @doc false
  def __schema__,
    do: [
      card: :path,
      connector: {:optional, :non_empty_string},
      mode: :mode,
      preferred_size: {:optional, :size},
      display: {:enum, [:on, :off]},
      wake_on_touch: {:optional, {:enum, [:single_tap, :double_tap, :long_press]}},
      backlight: {:record, Emerge.Config.Backlight},
      hotplug_interval_ms: {:optional, :positive_integer}
    ]
end
