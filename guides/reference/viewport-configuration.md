# Viewport configuration

Return `%Emerge.Config{}` from `mount/1` to choose your output and renderer settings.
For an upgrade from 0.4.2, start with [Migrate to 0.5.0](../migrations/0.5.md).

## Config groups

You only need to set the fields you customize. The others use defaults.

- `output` — a `Window`, `DRM` display or `Headless` frame receiver.
- `renderer` — drawing API, frame-rate limit and renderer cache.
- `input` — raw input subscriptions, scroll distance and cursor appearance.
- `assets` — application asset root, fonts, image caches and filesystem access.
- `diagnostics` — measurements and extra logging.
- `runtime` — health checks, startup retries and custom renderer selection.

These records live under `Emerge.Config`. Their module docs describe each field,
its default and whether it can change while the viewport is running.

## Desktop window

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

## Direct DRM display

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

## Headless binary frames

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

## Headless PRIME frames

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

## Structs and shorthand

Use explicit structs for `output` and `Headless.frames` to choose a variant.
Other records accept either structs or keyword shorthand. These are equivalent:

```elixir
%Emerge.Config{input: %Emerge.Config.Input{events: [:key]}}
%Emerge.Config{input: [events: [:key]]}
```

Unknown fields and invalid values are errors. New records use defaults for omitted
fields, not values from a previous config. Lists replace rather than append.
The callback's previous config always contains nested structs, even if mount
used shorthand.

## Change settings while running

Mount-only configuration needs no extra callback. To derive settings from local
state, optionally implement `config(previous, state)`; use `config(previous)`
when no local state is needed. Implement only one arity.

```elixir
@impl Viewport
def config(previous, state) do
  output = %{previous.output | title: state.title}
  %{previous | output: output}
end
```

Call `Viewport.rerender(state)` after updating state, as for UI changes. The config
callback runs before rendering, including when the UI tree is unchanged. It also
runs while a DRM display is off. It receives your latest requested config, not a
hardware readback. Update existing records to preserve their other fields.

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

Viewports infer `assets.otp_app` where possible. Standalone `EmergeSkia.start/1`
callers must supply it. See [Use assets](../tutorials/use_assets.md).

Use `Diagnostics.stats: true` for measurements read through `EmergeSkia.stats/2`.
Use `Diagnostics.logs: [:stats]` for printed summaries instead; this also enables
collection. macOS supports summaries, not the stats API. See
`Emerge.Config.Diagnostics` for categories and examples.

Build-time backend selection is separate from runtime config. For uncommon
backend selections or source builds, see
[Build the native renderer](native-renderer-builds.md).
