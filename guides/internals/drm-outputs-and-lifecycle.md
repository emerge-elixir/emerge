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

# Options returned from your viewport's mount callback:
[otp_app: :my_app, backend: :drm, drm_card: card,
 drm_output: output.name, drm_mode: mode]
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

## Two viewports on one GPU

Pass different discovered output names in the mount options of two viewport children.
If they use the same module, give their child specs distinct IDs:

```elixir
children = [
  Supervisor.child_spec({MyApp.Screen, left_options}, id: :left),
  Supervisor.child_spec({MyApp.Screen, right_options}, id: :right)
]
Supervisor.start_link(children, strategy: :one_for_one)
```

`MyApp.Screen.mount/1` must pass those rendering options through in its return value.
The application chooses its own supervision strategy; the library does not impose one.
For Vulkan, also select the intended GPU with `vulkan_drm_node` as usual.

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
fresh diff state and full tree upload. Callback state and PID stay intact. Retries are
bounded (20, with 250 ms–5 s exponential backoff). Session-specific relays fence old
input, close and heartbeat messages. A retirement acknowledgement drains queued native
close requests before recovery, preserving normal close and close-override behavior.
The video endpoint is published only after initial
upload and uses direct lookup/submission, without a lifecycle call per frame.

Callback death ends that public viewport instance and requests native cleanup even if
another process retains its renderer handle. The application decides whether to start
a new callback instance. A failure of private lifecycle supervision also ends the
public instance; it does not silently replace its PID.

Low-level native callers may supply `owner: self()` to `EmergeSkia.start/1`. Callers
without an owner retain responsibility for stopping their renderer.

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

## Opt-in hardware test

The test acquires master and modesets the first two connected outputs of the **explicitly
selected card**. Never run it against an in-use desktop GPU unintentionally. Build a
DRM-capable NIF first, then run:

```sh
EMERGE_DRM_TEST_CARD=/dev/dri/by-path/your-test-gpu-card \
EMERGE_DRM_TEST_API=opengl \
mix test test/emerge_skia/drm_hardware_test.exs --include drm_hardware
```

Use `EMERGE_DRM_TEST_API=vulkan` for the Vulkan presenter. The test covers simultaneous
presentation, duplicate rejection, repeated independent renderer replacement, callback
kill with a retained handle, and output reacquisition while the sibling keeps rendering.
