# Video interoperability architecture

Emerge renders video elements from viewport-local atom targets. Applications
submit storage-neutral `%VideoInterop.Frame{}` values directly to a running
viewport:

```elixir
video([width(fill()), height(fill()), image_fit(:contain)], :camera)

:ok = Emerge.submit_video_frame(viewport, :camera, frame)
```

Targets are names, not renderer handles. They carry no dimensions, mode, native
resource, or lifecycle. Layout determines the video element's bounds. The
submitted frame supplies only the source aspect ratio used by `image_fit`.

## Dependency boundaries

| Project | Owns | Does not own |
|---|---|---|
| `video_interop` | Frame and format schemas, binary and DMA-BUF storage, validation, leases, native fd preparation and retirement | Rendering or transport frameworks |
| `membrane_video_interop` | Bounded Membrane source/sink transport and RawVideo conversion | Emerge behavior or native imports |
| Emerge | Atom targets, scene visibility, direct submission, CPU conversion, GPU import, composition, renderer shutdown | Generic Membrane elements |
| Application | Pipeline topology, target names, processing, supervision, diagnostics | Native ownership bookkeeping |

`video_interop` has no Membrane dependency. `membrane_video_interop` has no
Emerge dependency; its sink calls a configured MFA.

## Storage contract

### Owned binary frames

`%VideoInterop.Binary{}` contains an immutable BEAM binary and one plane with an
offset and row stride. Stable formats are RGBA8888, RGB888, Gray8, Gray2, and
BW1. Gray2 and BW1 rows are MSB-first and independently strided; BW1 declares
whether one means black or white.

Binary frames:

- use implicit synchronization;
- have `lease: nil`;
- remain alive through ordinary BEAM binary ownership;
- are converted to RGBA once when submitted to Emerge.

### Borrowed DMA-BUF frames

`%VideoInterop.DMABuf.Descriptor{}` contains process-local borrowed fd integers,
object allocation sizes, modifiers, layers, and plane layouts. Native consumers
validate and duplicate every retained fd with `FD_CLOEXEC` before the NIF call
returns.

DMA-BUF frames require a unique lease holder. Claiming native ownership moves
release responsibility into a lifecycle-owned dispatcher. The frame retires
only after the renderer and GPU no longer read the imported storage.

Acquire synchronization is either implicit or an owned duplicate of a sync-file
fence. Format and per-frame synchronization policies must agree.

## Submission and visibility

`Emerge.submit_video_frame/3` bypasses the viewport GenServer mailbox through a
private registry that maps the viewport PID directly to its EmergeSkia renderer
handle. Registration occurs once after renderer startup and is removed before
renderer shutdown. This keeps high-rate media traffic out of application event
handling without renderer-module dispatch.

Every normal return consumes the supplied frame:

- hidden target: release immediately;
- visible target: replace the previous pending frame;
- accepted binary frame: retain owned RGBA until replacement or hiding;
- accepted DMA-BUF frame: retain its exact lease until renderer/GPU retirement;
- caller-owned rejection: Emerge releases before returning;
- transferred rejection: native code remains responsible for release.

The current render scene is authoritative. A target becomes active only when a
`video(..., target)` element is present. Repeated elements may read the same
latest frame but do not create new lease holders.

## Renderer flow

```text
Emerge.submit_video_frame(viewport, target, frame)
  -> lookup EmergeSkia renderer by viewport PID
  -> EmergeSkia.submit_video_frame(renderer, target, frame)
  -> EmergeSkia.video_frame_submit NIF
  -> VideoRegistry
       hidden: retire/drop
       visible: replace latest pending
  -> renderer thread synchronizes changed targets
  -> DrawPrimitive::Video composes the current image
  -> replaced/imported storage retires after safe completion
```

Binary submission validates the visible rectangle, stride, byte bounds, pixel
format, packed polarity, implicit synchronization, and lease-free ownership.
The NIF converts the visible rectangle to owned RGBA once. Renderer-local Skia
images are updated only when the registry generation changes.

DMA-BUF submission derives an immutable native stream contract from
`frame.format`. A target's stream is created lazily and replaced when that
contract changes. Prepared descriptors remain caller-owned until registry
admission succeeds; claimed frames carry exact release authority.

OpenGL imports use EGLImage/native-fence support where available. Vulkan import
supports exact validated packed and NV12 paths. Non-linear NV12 images and
linear NV12 buffers are copied plane-for-plane into bounded renderer-owned Y/UV
images before Skia composition, avoiding direct multi-planar wrapping. Vulkan
rendering and video import remain experimental pending pinned-RPi5 qualification.

## Headless output

Headless binary and PRIME modes both send:

```elixir
{message_tag, %VideoInterop.Frame{}}
```

The configured `headless.target` must be a live local PID.

Binary output is owned storage with no lease. PRIME output uses a bounded buffer
pool and leased DMA-BUF storage. If all slots are in flight, the configured
backpressure policy applies. A slot is reusable only after all holders retire
and backend synchronization proves it safe.

## Emerge-to-Emerge transport

Use Membrane rather than an application mailbox bridge:

```text
headless Emerge target PID
  -> Membrane.VideoInterop.Source
  -> optional processing
  -> Membrane.VideoInterop.Sink
  -> Emerge.submit_video_frame/3
```

A minimal sink callback is:

```elixir
def submit(frame, target, viewport) do
  Emerge.submit_video_frame(viewport, target, frame)
end
```

The source keeps at most one pending frame while demand is absent and releases a
replaced pending holder. The sink callback must consume the frame on every
normal return. Callback exceptions cause the sink to release the frame before
reporting the error.

Do not forward high-rate frames from an application's `handle_info/2`; doing so
mixes media flow with application events and makes backpressure unbounded.

## Shutdown

Ownership-safe shutdown follows this order:

1. stop new frame admission;
2. hide targets and retire pending/current frames;
3. stop the renderer and finish GPU work;
4. wait for every claimed lease to dispatch release;
5. close and join release dispatchers from dirty I/O or native lifecycle code;
6. stop the producer after its lease owner drains.

Release-dispatcher destructors never wait or detach worker threads. If shutdown
cannot prove all borrowed storage safe, `EmergeSkia.stop/1` returns an error and
the OS process must be cold-restarted before loading replacement native code.

## Diagnostics

`EmergeSkia.stats/2` reports video submission, replacement, inactive-drop,
import, synchronization, release, saturation, and Vulkan fault counters.
`renderer_stats_log: true` emits periodic summaries through the native log
relay. These diagnostics are optional and do not change frame ownership.
