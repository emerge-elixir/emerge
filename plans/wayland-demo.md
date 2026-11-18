# First Wayland demo: present the retained CPU raster

**Delivered on `refactor/retained-layers`.**
One ordinary Emerge viewport opens a 480×240 xdg-toplevel. Clicking the card
moves the same keyed occurrence between two lanes and increments a label. The
chips wrap on resize and have their own click targets. Closing the window stops
the viewport without exiting IEx. No defaults, elements, attributes, wire operations or public
renderer calls were added to make the demo work.

## Run the standalone demo

From the library checkout, on a Linux Wayland desktop:

```sh
cd ../wayland_cards
mix deps.get
iex -S mix                                # CPU (wl_shm)
# Or start a new session with an explicit GPU API:
WAYLAND_CARDS_API=vulkan iex -S mix       # or: WAYLAND_CARDS_API=opengl iex -S mix
```

The sibling project depends on `../emerge-retained-layers` by path. Its own
`config/config.exs` selects a source build with both GPU APIs. Run `mix test`
in that project for the cards regression; the library suite does not need the
sibling checkout.

**Standalone check:** On a private software compositor, `iex -S mix` displayed
the green card. Closing the window stopped the viewport and left IEx usable.
Explicit OpenGL and Vulkan requests produced their CPU fallback warnings in
that environment; this was not a hardware retest. The moved headless test
passed in the app. The library alone had 642/736 passing, the same 94 failure
names and 21 exclusions as the [GPU checkpoint](wayland-gpu-results.md).

## What runs

- The demo's `lib/wayland_cards.ex` uses ordinary `mount/1`, `render/1`,
  `Event.on_click/1`, `handle_info/2` and `Viewport.rerender/1`.
  Its `test/wayland_cards_test.exs` runs the same view, not a surrogate window
  renderer. It passed before the port. The v10
  NIF then failed the Wayland startup control with
  `only binary headless raster is supported in this build`.
- [Shared native session](../native/emerge_skia/src/session/mod.rs): one tree,
  raster, dispatcher, monitored owner, route stream and bounded wake. Accepted
  edits are applied first, then invalidated once against the final tree and
  recomputed. The binary headless sink sends `VideoInterop.Frame`; the Wayland
  sink never sends its pixels through the BEAM. Thirteen ordinary headless
  regressions passed after sharing the session and before linking the driver.
- [Wayland driver](../native/emerge_skia/src/session/wayland/mod.rs): SCTK and
  calloop on a dedicated thread. The initial bufferless commit requests a
  configure, which SCTK acknowledges. Configured logical size and integer
  scale determine the buffer size. Two [shm slots](../native/emerge_skia/src/session/wayland/shm.rs)
  convert premultiplied RGBA to native-endian ARGB8888; a slot cannot be
  written or resized while in flight. Configure, frame callback, release,
  frame-cap deadline, patch and stop wake the loop. Idle waits have no timer.
  Missing feature/compositor returns an error, not a headless window. Startup
  also interrupts a registry roundtrip if the compositor never answers.
- [Installation boundary](../native/emerge_skia/src/tree/paint.rs): preparing
  borrows the only raster. Pixels, hit attachments, pointer mapping and routes
  advance after submission succeeds. Reusing the installed raster can omit a
  commit only on the same surface at its submitted geometry. A frame callback,
  empty bootstrap commit, failed sink or equality of commands alone cannot
  publish. A failed preparation leaves the previous frame available; loss of
  the output transport fails the session. Surface commit is not scanout.
- [Seat input](../native/emerge_skia/src/session/wayland/pointer.rs): real
  enter/motion/left-button/leave events feed the shared dispatcher. Positions
  convert once using the installed scale, and the application gets local
  logical coordinates. Leave and focus loss cancel a held press. The close
  message uses the existing viewport relay; owner death wakes the same loop.

## Acceptance and limits

These observations precede the move from the script launcher to the sibling OTP app.

| Check | Observation |
| --- | --- |
| Source-build launcher | Private Linux `[wayland: [:raster]]` build; the unmodified launcher mapped `Retained cards` with `app_id=org.emerge.viewport`. Sway reported `visible: true`, size 480×240. Close exited with status 0. |
| Real compositor input/output | Isolated Sway 1.12, pixman, 720×500 headless **compositor output**. A virtual pointer on `wl_seat` drove the actual client; `grim` captured the desktop. `handle_input/2` saw surface `(17,17)` press/release; `handle_info/2` saw card-local `(5,5)`. `Moves: 1` appeared with the card on the right. Old spot missed; new spot moved it back, `Moves: 2`. No `send_input/2`, callback injection or renderer screenshot served as window acceptance. |
| Resize, scale and lifecycle | At 240×240, Coral wrapped to the second row. Clicking it produced `Selected: Coral` and changed its fill. At integer scale 2 a card click still reported `(5,5)`. No surface operations during four idle compositor captures. xdg close and killed viewport owner both finished native cleanup; a fresh viewport displayed the initial scene. |
| Native controls | Failed first submission, failed resized submission, superseded preparation and failed reuse preserve installed pixels, extent, attachment and hit location. Busy slots remain unavailable across resize; checked size, stride and native-endian premultiplied conversion have independent tests. Missing display and missing feature were separately observed; an unresponsive registry socket closed on startup timeout. |
| Builds and regressions | Release tests/build/strict Clippy: **41** Rust tests for Wayland-raster-only and for the prior default profile, **39** for headless-only. Headless-only demo/retained/identity regressions **21 passed**. Cargo/Mix format, forced warnings-as-errors compilation, strict Credo and Dialyzer passed. |
| Whole Mix suite | Matched default profile **640/736 passed, 96 failed, 21 excluded**: the exact same 96 names as the [v10 checkpoint](stable-node-ids-results.md), plus one passing demo test. Wayland CPU-only **640/735 passed, 95 failed, 1 skipped, 21 excluded**: the existing CPU-profile PRIME skip, not a fix or a new exclusion. |
| Linux/glibc allocator control | In all three profiles the measured cold-after-reserve and fitting tree edits through draw-input preparation remained `[0,0,0]`. Initial `[72449,9785,72174]`, one-over text `[0,6,0]`; one-over topology `[0,57,0]` or `[1,57,1]`. These count alloc/realloc/free, **not** rasterization, shm copies, compositor work or delivery. |

Reproduction evidence is separate from the earlier checkpoints:
`/workspace/emerge-investigation-artifacts/wayland-cards-I2MZzO`. See
`controls.exs`, `controls/01-open.ppm` through `08-reopen.ppm`, `logs/final-desktop`,
`logs/direct-source`, `launcher-mapped`, `logs/profile-*`, `logs/stalled`,
`logs/final-heap-*` and `logs/{matched,cpu}-delta`. `/proc` mappings were
checked against the actual binaries. Wayland-raster-only NIF SHA-256:
`8183e9e729f3052d57df15569b115795113fe9208cbf0523fb65750edd988cf0`.

The old [presenter](../native/emerge_skia/src/backend/wayland/runtime.rs) is a
reference, not a second layout, paint or input engine. No GPU, fractional
scale, physical scanout, keyboard/IME interaction, animation, scrolling,
external assets, product-wide correctness or timing result is claimed.
