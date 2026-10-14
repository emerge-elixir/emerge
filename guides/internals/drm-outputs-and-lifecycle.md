# DRM outputs and viewport lifecycle

Emerge has no application callback or automatically started runtime supervisor. Start
viewports explicitly in your application's supervision tree. Each viewport creates its
own private lifecycle supervisor; the returned PID remains the callback GenServer, not
a supervisor or proxy. Names, direct messages, calls, links, event callback `self()`,
Solve subscriptions and code-reload membership keep their existing meanings.

## Discover and select an output

Build with DRM support, then query the **KMS primary node**, preferably using a stable
`/dev/dri/by-path/…-card` path:

```elixir
card = "/dev/dri/by-path/pci-0000:6c:00.0-card"
{:ok, outputs} = EmergeSkia.drm_outputs(drm_card: card)
output = Enum.find(outputs, &(&1.connected and &1.name == "DP-4"))
mode = Enum.find(output.modes, & &1.preferred) || hd(output.modes)

# Configuration returned from your viewport's mount callback:
selected = %Emerge.Config.DRM{card: card, connector: output.name, mode: mode}
{:ok, %Emerge.Config{output: selected, renderer: [api: :opengl]}}
```

Discovery creates no renderer, performs no modeset and requires no `otp_app`. It returns
connector names/IDs, connection state, and advertised modes with exact timing IDs,
dimensions, fractional refresh rate, preferred and interlaced flags. It is a snapshot:
startup revalidates the selected connector and mode. A mode map or its `id` is accepted;
a mode requires an output. Explicit selections never silently migrate to another
connector or substitute a different mode. Without selectors, automatic selection
retains size/preferred-mode heuristics and chooses an available output.

Discovery and startup return errors when DRM support, permissions, master ownership,
leasing, the connector, mode or compatible scanout resources are unavailable.

## CPU-only DRM

`config :emerge, compiled_backends: [drm: [:raster]]` builds a DRM presenter with
no GBM, EGL or GPU dependencies. Return `%Emerge.Config{output: %Emerge.Config.DRM{},
renderer: [api: :raster]}` from mount; a CPU-only build also selects raster for
`renderer.api: :auto`. Use the matching `drm_raster` artifact when available, or
build from source; see the [native build guide](../reference/native-renderer-builds.md).

Skia draws into an available DRM dumb buffer with its actual row pitch. Three
XRGB8888 buffers hold current, in-flight and at most one prepared frame.
Nonblocking atomic commits request page-flip events; accepting a commit does not
release its predecessor. Only completion permits normal buffer reuse. Teardown
uses a blocking disable barrier and retains uncertain ownership on failure.
No per-frame BEAM transfer or custom output NIF is involved. Screenshots
copy/convert to RGBA on demand and publish after the corresponding flip.
Cursor drawing is software-only; CPU video works, PRIME import does not.
Atomic KMS, DRM leasing and XRGB8888 dumb-buffer support remain required.

Raster `renderer.raster_present: :auto` means CPU on DRM. Explicit `:gpu_upload`
keeps the old OpenGL upload path when that feature is compiled. A disconnect or
removed mode stops the raster backend after safe detachment; viewport lifecycle
recovery then reacquires the selected output. Failed detachment quarantines its
buffers and lease instead of permitting unsafe reuse.

## Two viewports on one GPU

Pass different discovered output records as application arguments to two viewport children.
If they use the same module, give their child specs distinct IDs:

```elixir
children = [
  Supervisor.child_spec({MyApp.Screen, [output: left_output]}, id: :left),
  Supervisor.child_spec({MyApp.Screen, [output: right_output]}, id: :right)
]
Supervisor.start_link(children, strategy: :one_for_one)
```

Here `left_output` and `right_output` are `%Emerge.Config.DRM{}` records.
`MyApp.Screen.mount/1` translates the application argument into a typed config:

```elixir
def mount(opts) do
  {:ok, %Emerge.Config{output: Keyword.fetch!(opts, :output), renderer: [api: :opengl]}}
end
```

The application chooses its own supervision strategy; the library does not impose one.
For explicit Vulkan, also select the intended GPU with `renderer.device`, independently
of `output.card`. Live connector/mode changes are currently rejected.

A shared master is keyed by device identity, not the path spelling. Each renderer gets
an independently readable DRM lease and reserves its connector, encoder, CRTC, primary
plane and optional cursor plane. Aliased paths cannot bypass reservations. Duplicate
output selection fails, including while an earlier session is stopping or quarantined.
One viewport's normal stop/restart does not drop the shared master or consume the
sibling's page-flip events. GL dispatch is initialized once rather than rewriting a
process-global function table while siblings render. Input-device routing is separate
from output selection.

Lease revocation is explicit after safe teardown: Mesa can retain a session's DRM file
inside GPU state shared with another context, so merely closing our FD is insufficient.
Failed revocation keeps the reservation pinned rather than admitting a replacement.

## Cleanup and recovery

For native renderers, the viewport is a monitored owner before startup. Essential
worker exits trigger cleanup independently of the callback mailbox or garbage
collection. Native startup waits have a 15-second deadline, followed by up to five
seconds waiting for rollback; a blocked cleanup worker retains its resources.

An unexpected stopped renderer is replaced in place after **proven cleanup**, with a
fresh diff state and full tree upload while on. Initially-off/recovered-off owners
are acquired and routed without rendering or uploading a tree. Callback state,
latest desired config and PID stay intact. Retries are
bounded (20, with 250 ms–5 s exponential backoff). Session-specific relays fence old
input, close and heartbeat messages. A retirement acknowledgement drains queued native
close requests before recovery, preserving normal close and close-override behavior.
The video endpoint is published only after initial
upload and uses direct lookup/submission, without a lifecycle call per frame.

Callback death ends that public viewport instance and requests native cleanup even if
another process retains its renderer handle. The application decides whether to start
a new callback instance. A failure of private lifecycle supervision also ends the
public instance; it does not silently replace its PID.

`EmergeSkia.start/1` accepts only `%Emerge.Config{}`. Native owner/baseline handles
are private lifecycle plumbing, not config fields. Standalone callers are responsible
for stopping their renderer; normal viewports manage ownership automatically.

```elixir
EmergeSkia.stop(renderer, timeout: 5_000)
{:ok, status} = EmergeSkia.renderer_status(renderer)
# status: session_id, state, failure, cleanup_complete
```

`running?/1 == false` does **not** mean cleanup finished. Concurrent/repeated stops
observe the same completion. A timeout does not cancel cleanup, free ownership, or
permit reuse. Status states are `:running`, `:stopping`, `:stopped`, and `:quarantined`;
failures include a reason, scope, and recovery classification. Headless PRIME also
requires the lease owner and release dispatcher to drain before reporting completion.
macOS host sessions do not yet implement native owner/status/bounded-stop contracts;
they retain their existing host transport behavior.

Uncertain GL/KMS teardown retains the affected output and its owners. Existing Vulkan
process-wide terminal quarantine remains in force: do not hot-reload the NIF or bypass
it to obtain a restart. GPU/device loss may affect siblings; sibling isolation is not
a promise of recovery from a shared hardware failure. Worker monitoring detects exits,
not a GPU/driver hang, and presenter-panic/hot-unplug fault injection is not yet validated.

## Declarative display power and brightness

`output.display` and `output.backlight.brightness` are live for explicit OpenGL,
Vulkan and raster (CPU or GPU-upload presentation). Automatic API power controls
remain rejected. GPU-upload raster requires the OpenGL DRM build feature and
reuses its existing application-frame gate, KMS detach, GL completion and resource
retirement; it does not add a separate power or video-import policy.
Automatic backlight selection requires a proven connector association; `device: nil`
opts out, and an explicit sysfs path pins a particular device. Brightness is a finite
`0..1` request; zero is not display-off, and `nil` restores the original requested
level retained across renderer recovery.

Off retains the public renderer, lease/backlight ownership, tree and desired settings,
but ignores native UI input and destroys the presenter. On creates a fresh presenter
and enables a managed backlight after its first application presentation. There is no
GPU restoration, strict config/frame provenance or physical no-flash guarantee.
Vulkan retirement orders backlight suppression, GPU idle, blocking KMS disable
and old-event drain before destroying scanout/import owners. A managed child
never restores inherited KMS state or publishes drained frames as presentations.
Uncertain ownership remains process-terminal quarantine, not a recoverable off/on.
Identical video capabilities may be republished for retained streams on recreation;
changed capabilities still fail. Vulkan panel/optical qualification remains open.
Applications provide their independent wake policy. See
[config reconciliation](config-reconciliation.md) and the
[configuration reference](../reference/viewport-configuration.md).

## Opt-in GPU power test

This single-output test modesets only the explicitly selected card/connector.
`EMERGE_DRM_TEST_POWER_RENDERER=vulkan` (the default) uses an independently selected
Vulkan node; `gpu_upload` uses explicit raster GPU-upload via OpenGL/GBM. It does
not manage a backlight unless `EMERGE_DRM_TEST_BACKLIGHT` explicitly names one.
Use a matching source-built DRM profile and an authorized non-desktop output.
For Vulkan, enable the Khronos synchronization validation layer:

```sh
EMERGE_DRM_TEST_CARD=/dev/dri/by-path/your-test-card \
EMERGE_DRM_TEST_CONNECTOR=DSI-1 \
EMERGE_DRM_TEST_POWER_RENDERER=vulkan \
EMERGE_DRM_TEST_VULKAN_NODE=/dev/dri/by-path/your-test-render-node \
EMERGE_VULKAN_SYNC_VALIDATION=1 \
mix test test/emerge_skia/drm_gpu_power_hardware_test.exs --include drm_hardware
```

For GPU upload, select an OpenGL-enabled DRM source profile, use
`EMERGE_DRM_TEST_POWER_RENDERER=gpu_upload`, and omit the Vulkan-node/validation
variables. The test requires hardware GL (not llvmpipe/softpipe) and verifies
kernel enabled/disabled state, captured CPU-raster content and stable ownership.

Only for an authorized panel backlight, also set
`EMERGE_DRM_TEST_BACKLIGHT=/sys/class/backlight/your-panel`. The test covers initial
off, delayed first upload, brightness-only updates, repeated/rapid off/on, bounded
descriptors, extended-off frame counts, stop without upload and output
reacquisition. Stats/config completion do not prove optical no-flash behavior:
observe the panel and retain full native validation output,
cleanup status and resource/power measurements. Reconnect, video and sibling
acceptance also require device testing.

## Opt-in hardware test

The test acquires master and modesets the first two connected outputs of the **explicitly
selected card**. Never run it against an in-use desktop GPU unintentionally. Build a
DRM-capable NIF first, then run:

```sh
EMERGE_DRM_TEST_CARD=/dev/dri/by-path/your-test-gpu-card \
EMERGE_DRM_TEST_API=opengl \
mix test test/emerge_skia/drm_hardware_test.exs --include drm_hardware
```

Use `EMERGE_DRM_TEST_API=vulkan` for the Vulkan presenter or `raster` for CPU DRM. The test covers simultaneous
presentation, duplicate rejection, repeated independent renderer replacement, callback
kill with a retained handle, and output reacquisition while the sibling keeps rendering.

A separate single-display CPU test checks colors, RGBA screenshots, updates and
output release/reacquisition. It also modesets the explicitly selected card:

```sh
EMERGE_DRM_TEST_CARD=/dev/dri/by-path/your-test-gpu-card \
  mix test test/emerge_skia/drm_raster_hardware_test.exs --include drm_hardware
```

Build the `drm: [:raster]` profile before running it to validate the no-GPU path.
