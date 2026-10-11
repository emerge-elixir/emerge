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
  - `backlight` — `Emerge.Config.Backlight`, defaulting to automatic association.
    Use it to select a panel backlight and request brightness.
  - `hotplug_interval_ms` — connector hotplug polling interval in milliseconds;
    defaults to `750`. Set it to `nil` to disable polling for built-in panels.

  Discover connector names and modes with `EmergeSkia.drm_outputs(drm_card: path)`.

  ## Changing settings

  `display` and backlight brightness can change through `config` with an explicit
  OpenGL renderer or CPU raster renderer. For raster, use `raster_present: :cpu`
  or its `:auto` default. Vulkan and raster GPU upload do not support these controls.

  Changing the card, connector, mode, size hint, backlight device or hotplug polling
  interval requires a new viewport. Applications using off/on need an independent
  wake source, such as a power-button reader; input from the blanked display cannot
  wake it.
  """
  defstruct card: "/dev/dri/card0",
            connector: nil,
            mode: nil,
            preferred_size: nil,
            display: :on,
            backlight: %Emerge.Config.Backlight{},
            hotplug_interval_ms: 750

  @typedoc "Desired display state, not application or system power state."
  @type display :: :on | :off
  @type t :: %__MODULE__{
          card: String.t(),
          connector: String.t() | nil,
          mode: String.t() | map() | nil,
          preferred_size: {pos_integer(), pos_integer()} | nil,
          display: display(),
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
      backlight: {:record, Emerge.Config.Backlight},
      hotplug_interval_ms: {:optional, :positive_integer}
    ]
end
