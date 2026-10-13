# Viewport configuration

Emerge 0.5 uses one `%Emerge.Config{}` for viewport and standalone renderer startup.
Return it from `mount/1`; optionally declare live changes through `config/1` or
`config/2`. Flat keyword configs, the `emerge_skia`/`viewport` namespaces and public
numeric input masks are removed. See [Migrate to 0.5](../migrations/0.5.md).

Application arguments passed to `start_link/1` remain keyword lists that your
`mount/1` interprets. GenServer options (`name`, `timeout`, `debug`, `spawn_opt`,
`hibernate_after`) configure the process, not the renderer.

## Six groups

| Field | Record | Responsibility |
|---|---|---|
| `output` | `Emerge.Config.Window`, `DRM` or `Headless` | Where frames go |
| `renderer` | `Emerge.Config.Renderer` | Drawing API/device, raster presentation, cap and paint cache |
| `input` | `Emerge.Config.Input` | Raw observers, scroll-step distance and cursor policy |
| `assets` | `Emerge.Config.Assets` | App asset root, fonts, runtime paths, decode policy and image caches |
| `diagnostics` | `Emerge.Config.Diagnostics` | Structured stats, extra logs and diagnostic GPU waits |
| `runtime` | `Emerge.Config.Runtime` | Health/retry policy and advanced custom adapters |

Each record's module documentation lists all fields, defaults and types. `%Emerge.Config{}`
uses a platform-default desktop window. Fixed-shape groups accept typed keyword
shorthand; output and binary/PRIME frame variants require explicit structs. Unknown
or duplicate fields, wrong records and invalid values are rejected with a field path.
Struct construction itself does not validate: normalization happens at the runtime boundary.

## Desktop window

<!-- emerge-config:window -->
```elixir
%Emerge.Config{
  output: %Emerge.Config.Window{title: "My App", size: {1024, 768}},
  renderer: [api: :raster],
  input: [events: [:key, :resize]]
}
```

`Window.backend: :auto` chooses the platform default (macOS or Wayland).
`renderer.api` is independent: macOS supports `:auto`, `:metal` and `:raster`;
Wayland supports `:auto`, `:opengl`, `:raster` and `:vulkan` when compiled.
`Window.size` is the requested initial surface size. `always_on_top` is a macOS
window option, not DRM output policy. Only the title is live in the window record.

## Direct DRM

<!-- emerge-config:drm -->
```elixir
%Emerge.Config{
  output: %Emerge.Config.DRM{
    card: "/dev/dri/card0",
    connector: "DSI-1",
    backlight: [device: "/sys/class/backlight/panel", brightness: 0.5],
    hotplug_interval_ms: nil
  },
  renderer: [api: :opengl]
}
```

The example backlight path must be replaced with your device's actual path.
`card` is the KMS primary node, preferably a stable `/dev/dri/by-path/...-card`
path. Query `EmergeSkia.drm_outputs(drm_card: card)` for connector names and exact
advertised mode IDs/maps; discovery remains a keyword query, not renderer startup.
An explicit `mode` requires `connector`. Explicit selections do not silently fall
back. `preferred_size` is only a mode-selection hint; the selected mode sets the
actual output dimensions. Connector/mode changes are startup-only in practice:
live changes are explicitly rejected.

The connector is polled every `hotplug_interval_ms` (default `750`) to detect hotplug; built-in panels can set it to `nil`.

For CPU-only presentation use `renderer: [api: :raster, raster_present: :cpu]`.
Raster's `:auto` route also selects CPU on DRM; `:gpu_upload` explicitly uses the
OpenGL-backed upload presenter. A compatible compiled backend and driver are
required. Explicit DRM Vulkan additionally requires an absolute `renderer.device`
path, independently of the scanout `card`.

### Display power and backlight

Power/brightness controls require **explicit `api: :opengl`** or
`api: :raster` with `raster_present: :auto | :cpu`. `api: :auto` is not an explicit
power-support declaration; Vulkan and raster GPU-upload power remain rejected.

- `display: :off` ignores native UI input, suppresses a managed backlight and
  retires the presenter. The public owner, tree, desired settings and output/
  backlight reservations survive; GPU resources do not.
- `display: :on` creates a fresh presenter, requests the latest retained tree and
  enables the managed backlight after its first application presentation.
- Initially-off viewports still acquire ownership/routing, without a render
  callback or tree upload. Config reconciliation and application messages continue.
- `backlight.device: :auto` requires a proven, unambiguous connector association.
  No associated device means KMS-only operation unless brightness was requested;
  ambiguity is an error. `nil` opts out; an absolute path binds explicitly.
- `brightness` is finite `0..1`, rounded to the device's integer range. Zero is
  brightness zero, **not display-off**. `nil` restores the original requested
  brightness captured before suppression and retained across renderer recovery.
- Brightness changes while off retain intent without illumination. Clean stop
  leaves a managed lamp dark. Unsafe teardown retains/quarantines ownership.

Applications own power-button/wake policy. Off does not suspend the BEAM, OS,
networking, application timers or camera producers. Software support is not a
physical no-flash, no-stale-input or panel-driver timing guarantee. See
[config reconciliation](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/config-reconciliation.md) for ownership and
completion details.

## Headless owned binary frames

<!-- emerge-config:binary -->
```elixir
%Emerge.Config{
  output: %Emerge.Config.Headless{
    target: self(),
    size: {400, 300},
    frames: %Emerge.Config.BinaryFrames{pixel_format: :bw1, dither: true}
  },
  renderer: [api: :raster, target_fps: 30],
  assets: [otp_app: :my_app]
}
```

`target` must be a live local PID; it is not inferred. Frames arrive as
`{frame_message, %VideoInterop.Frame{}}`, with `:emerge_skia_frame` as the default
tag. Binary frames own their storage and have no lease. Formats are `:rgba8888`,
`:bgra8888`, `:rgb888`, `:gray8`, `:gray2` and `:bw1`. Packed rows are MSB-first;
`bw1_polarity` chooses whether a set bit is black or white. Dithering is supported
for raster BW1/Gray2 output, not arbitrary formats or GPU APIs.

## Headless PRIME frames

<!-- emerge-config:prime -->
```elixir
%Emerge.Config{
  output: %Emerge.Config.Headless{
    target: self(),
    size: {640, 480},
    frames: %Emerge.Config.PrimeFrames{
      device: "/dev/dri/renderD128",
      max_in_flight: 2,
      on_backpressure: :drop_new
    }
  },
  renderer: [api: :opengl, target_fps: 30],
  assets: [otp_app: :my_app]
}
```

PRIME is a Linux DMA-BUF output path for a compatible GPU build/device, not
CPU raster. The recipient must retire the frame's lease using `VideoInterop`.
When all pool slots are held, `:drop_new` drops new output rather than creating
unbounded storage. PRIME output is separate from video frames submitted *into*
an Emerge viewport. See [video interoperability](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/video-interop-architecture.md).

## Declare live changes

Implement neither config callback or exactly one arity. `config(previous)`
receives previous config; `config(previous, state)` also receives viewport state.
Both return a complete root config, not a tuple or patch.

```elixir
@impl Viewport
def config(%Emerge.Config{output: %Emerge.Config.Window{} = output} = previous, state) do
  output = %Emerge.Config.Window{output | title: state.title}
  %Emerge.Config{previous | output: output}
end
```

The first invocation receives normalized mount config before renderer creation.
Later invocations run before render during coalesced rerenders, including config-only
updates and while a DRM display is off. Nested records in `previous` are always
structs. Previous means **desired intent**, not hardware readback or the last
confirmed revision. Recovery replays that latest intent, not mount defaults.

New records replace old records. For example, assigning `backlight: [brightness: 0.2]`
resets `device` to its default `:auto`; it does not preserve a previously explicit
path. Use one typed struct update at a time when preservation is intended:

```elixir
@impl Viewport
def config(%Emerge.Config{output: %Emerge.Config.DRM{} = output} = previous, state) do
  backlight = %Emerge.Config.Backlight{output.backlight | brightness: state.brightness}
  output = %Emerge.Config.DRM{output | display: state.display, backlight: backlight}
  %Emerge.Config{previous | output: output}
end
```

Lists (events, fonts, allowlists, logs) replace, never append implicitly. An unchanged
normalized config does no native work. A cap/title/input/brightness change does not
require changing the UI tree. Application state changes still request a cycle with
`Viewport.rerender(state)`.

### Live/static support

| Field | Current support |
|---|---|
| `input.events` | Live raw-observer replacement; does not disable element interactions |
| `runtime.health_check_interval_ms` | Runtime-owned live update |
| `Window.title` | Live on Wayland and macOS |
| `renderer.target_fps` | Startup/live on headless and Wayland only; `nil` restores backend pacing |
| `DRM.display`, `DRM.backlight.brightness` | Live for explicit OpenGL and CPU raster only |
| `DRM.connector`, `DRM.mode` | Live changes rejected |
| Other fields | Startup-only; create a new viewport/session |

A frame cap bounds render admissions, not physical refresh, callbacks, producers or
application timers. Already admitted work can finish; idle scenes remain idle and
missed deadlines do not cause catch-up bursts. See [frame pacing](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/frame-pacing.md).
Vulkan is an available compiled API, but the separately recorded Wayland acquire
synchronization defect remains deferred; functional test passes do not qualify it.

Callback/schema/capability failures preserve previous intent. Operational configure
failures preserve desired intent for retry/recovery; a subsequent rerender retries
the complete live snapshot. Native admission returns a session/revision token, not
proof of presentation. `EmergeSkia.config_status/1` distinguishes pending/applied/
failed revisions; applied history is not rollback or independent hardware readback.
macOS host revision reporting is not implemented. See
[config reconciliation](https://github.com/emerge-elixir/emerge/blob/main/guides/internals/config-reconciliation.md).

## Assets, diagnostics and advanced runtime

`assets.otp_app` is inferred from a viewport module where possible. Standalone
`EmergeSkia.start(%Emerge.Config{})` requires it explicitly. Fonts, source policy,
decode settings and cache budgets are startup-only and renderer-local. See
[Use assets](../tutorials/use_assets.md), `Emerge.Config.Assets` and its nested records.

`diagnostics: [stats: true]` collects structured counters/timings for
`EmergeSkia.stats/2`, without periodic stats output. `diagnostics: [logs: [:stats]]`
enables both collection and summaries; using both is redundant for logging.
Stats logs reset the same native window read by the API. macOS supports summaries,
not that API. Other log categories do not enable collection. `force_gpu_finish`
adds diagnostic blocking DRM EGL waits and distorts timings; keep it false normally.
See `Emerge.Config.Diagnostics` for categories, destinations and backend limits.

`Emerge.Config.Runtime` contains health intervals, DRM startup retry policy, and
custom `adapter`/`adapter_opts`. Native owner/baseline resources are internal, not
config fields. Custom adapters implement the new typed startup and full-snapshot
configure contract; see the [migration guide](../migrations/0.5.md#custom-renderer-adapters).

Compile-time backend/artifact selection is separate from runtime config; a record
cannot enable code absent from the NIF. See [Build the native renderer](native-renderer-builds.md).
