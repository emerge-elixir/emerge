defmodule Emerge.Config do
  @moduledoc """
  Configuration returned by a viewport's `mount/1` callback, or passed to
  `EmergeSkia.start/1` when using a renderer directly.

  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Window{title: "My App"},
    renderer: %Emerge.Config.Renderer{api: :raster}
  }
  ```

  ## Groups

  - `output` — `Emerge.Config.Window` (default), `Emerge.Config.DRM` or
    `Emerge.Config.Headless`. Chooses where frames go.
  - `renderer` — `Emerge.Config.Renderer`. Chooses how frames are drawn.
  - `input` — `Emerge.Config.Input`. Raw input subscriptions default to none;
    ordinary UI element interactions still work.
  - `assets` — `Emerge.Config.Assets`. Application asset paths, fonts and images.
  - `diagnostics` — `Emerge.Config.Diagnostics`. Optional measurements and logs.
  - `runtime` — `Emerge.Config.Runtime`. Health checks and advanced renderer options.

  All groups have defaults. Use explicit structs to choose the `output` and
  headless frame variant. Other records also accept keyword shorthand:

  ```elixir
  %Emerge.Config{input: [events: [:key, :resize]]}
  ```

  A viewport infers the asset application where possible. Direct renderer callers
  must supply `assets.otp_app`; headless output also needs a live target PID.

  ## Change settings while running

  An optional `c:Emerge.config/1` or `c:Emerge.config/2` callback receives the previous
  requested config, with all nested records expanded to structs. Return a config
  with the changes you want:

  ```elixir
  def config(previous, state) do
    output = %{previous.output | title: state.title}
    %{previous | output: output}
  end
  ```

  Updating an existing record preserves its other fields. Supplying a new record
  uses defaults for omitted fields; it does not merge with the previous record.
  Lists replace rather than append. Not all fields can change while running:
  each module explains which changes need a new viewport and its backend limits.

  See [viewport configuration](Emerge.html#module-viewport-configuration) for output,
  build settings, target defaults and complete device examples,
  [raw input](handle_input.html) for callbacks, and
  [Migrate from 0.4.2 to 0.5.0](0-5.html) for the breaking changes.

  ## Output examples

  ### Desktop window

  <!-- emerge-config:window -->
  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Window{title: "My App", size: {1024, 768}},
    renderer: %Emerge.Config.Renderer{api: :raster},
    input: %Emerge.Config.Input{events: [:key, :resize]}
  }
  ```

  `Window.backend` defaults to the platform's window backend: Wayland on Linux,
  macOS on macOS. The drawing API is selected separately. Window titles can change
  while running; other window settings require a new viewport.

  ### Direct DRM display

  <!-- emerge-config:drm -->
  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.DRM{
      card: "/dev/dri/card0",
      connector: "DSI-1",
      backlight: %Emerge.Config.Backlight{
        device: "/sys/class/backlight/panel",
        brightness: 0.5
      },
      hotplug_interval_ms: nil
    },
    renderer: %Emerge.Config.Renderer{api: :opengl}
  }
  ```

  Replace the backlight path with your panel's actual device. Discover connectors
  and modes with `EmergeSkia.drm_outputs(drm_card: path)`.

  The connector is polled every `hotplug_interval_ms` milliseconds (default `750`)
  to detect hotplug; built-in panels can set it to `nil` to disable polling.

  For CPU raster, use `%Emerge.Config.Renderer{api: :raster, raster_present: :cpu}`.
  For Vulkan or raster GPU upload, replace the example's `renderer` with one of:

  ```elixir
  %Emerge.Config.Renderer{api: :vulkan, device: "/dev/dri/renderD128"}
  %Emerge.Config.Renderer{api: :raster, raster_present: :gpu_upload}
  ```

  Select the Vulkan device independently of `output.card`; both must support the
  chosen output. Display on/off and brightness can change with any of these
  explicit APIs. GPU upload uses the OpenGL DRM presenter and requires its build
  feature, even though raster draws the scene.

  Set `display: :off` to blank the display and ignore its UI input. Your application
  continues running, and brightness changes made while off apply on wake. Provide
  an independent wake source, such as a power-button reader. Brightness `0` is not
  display-off; brightness `nil` restores the original requested level. Off/on
  recreates the internal presenter while preserving the public renderer, viewport
  state and requested brightness baseline. GPU resources are not restored; managed
  illumination waits for the fresh application frame's presentation.

  See `Emerge.Config.DRM` and `Emerge.Config.Backlight` for selection and brightness.

  ### Headless binary frames

  <!-- emerge-config:binary -->
  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      size: {400, 300},
      frames: %Emerge.Config.BinaryFrames{pixel_format: :bw1, dither: true}
    },
    renderer: %Emerge.Config.Renderer{api: :raster, target_fps: 30},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  The target receives `{:emerge_skia_frame, %VideoInterop.Frame{}}`. These frames own
  their pixel data and need no release. Set `Headless.frame_message` to change the
  message tag. `BinaryFrames` describes the available pixel formats and dithering.

  ### Headless PRIME frames

  <!-- emerge-config:prime -->
  ```elixir
  %Emerge.Config{
    output: %Emerge.Config.Headless{
      target: self(),
      size: {640, 480},
      frames: %Emerge.Config.PrimeFrames{
        device: "/dev/dri/renderD128",
        max_in_flight: 2
      }
    },
    renderer: %Emerge.Config.Renderer{api: :opengl, target_fps: 30},
    assets: %Emerge.Config.Assets{otp_app: :my_app}
  }
  ```

  This Linux GPU output sends DMA-BUF frames. Release each frame's lease through
  `VideoInterop` when finished. When all output buffers are held, new frames are
  dropped. See `Emerge.Config.PrimeFrames` for device selection and pool size.

  ## Live fields and backend limits

  After updating state, call `Emerge.rerender/1` to schedule configuration and UI
  reconciliation. The callback receives your latest requested configuration, not
  a hardware readback, and runs even when the tree is unchanged or a DRM display
  is off. Update existing records to preserve unrelated fields.

  For a DRM brightness update:

  ```elixir
  backlight = %{previous.output.backlight | brightness: new_level}
  output = %{previous.output | backlight: backlight}
  %{previous | output: output}
  ```

  ### Settings that can change

  - Raw input subscriptions and the health-check interval.
  - Window titles on Wayland and macOS.
  - Frame-rate limits on headless and Wayland.
  - DRM display on/off and brightness with explicit OpenGL, Vulkan or raster
    (CPU or GPU-upload presentation).

  ### Settings that require a new viewport

  Output type/device/connector/mode, size, drawing API/device, caches, cursor
  appearance, assets, diagnostics, startup retry policy and adapter settings.
  Unsupported changes return an error instead of restarting your viewport.

  ### Backend limitations

  DRM and macOS do not support frame-rate caps, even at startup. Automatic DRM API
  selection does not support display power/brightness controls. GPU-upload raster
  requires a build with the OpenGL DRM presenter. Off/on destroys and recreates the
  internal presenter; it does not restore GPU state. Device-specific backlight and
  optical behavior still require hardware qualification; display blanking is not a
  physical monitor power-off guarantee.

  ## Assets and diagnostics

  Viewports infer `assets.otp_app` where possible; standalone `EmergeSkia.start/1`
  callers must supply it. See `Emerge.Config.Assets` and [use assets](use_assets.html).
  `Emerge.Config.Diagnostics.stats` enables measurements read by `EmergeSkia.stats/2`.
  `logs: [:stats]` prints summaries and also enables collection. The macOS host
  supports summaries, not the stats API; see `Emerge.Config.Diagnostics`.

  """

  alias Emerge.Config.{Assets, Diagnostics, Input, Renderer, Runtime, Window}

  defstruct output: %Window{},
            renderer: %Renderer{},
            input: %Input{},
            assets: %Assets{},
            diagnostics: %Diagnostics{},
            runtime: %Runtime{}

  @type output :: Window.t() | Emerge.Config.DRM.t() | Emerge.Config.Headless.t()
  @type t :: %__MODULE__{
          output: output(),
          renderer: Renderer.t() | Renderer.options(),
          input: Input.t() | Input.options(),
          assets: Assets.t() | Assets.options(),
          diagnostics: Diagnostics.t() | Diagnostics.options(),
          runtime: Runtime.t() | Runtime.options()
        }
  @typedoc "Validated configuration with all nested keyword records expanded into structs."
  @type normalized_t :: %__MODULE__{
          output:
            Window.normalized_t()
            | Emerge.Config.DRM.normalized_t()
            | Emerge.Config.Headless.normalized_t(),
          renderer: Renderer.normalized_t(),
          input: Input.normalized_t(),
          assets: Assets.normalized_t(),
          diagnostics: Diagnostics.normalized_t(),
          runtime: Runtime.normalized_t()
        }

  @doc false
  def __schema__ do
    [
      output: {:variants, [Window, Emerge.Config.DRM, Emerge.Config.Headless]},
      renderer: {:record, Renderer},
      input: {:record, Input},
      assets: {:record, Assets},
      diagnostics: {:record, Diagnostics},
      runtime: {:record, Runtime}
    ]
  end
end
