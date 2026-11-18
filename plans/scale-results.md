# Retained scale: implementation and checks

`Emerge.UI.scale/1` now works on the retained shapes. It changes subtree layout,
not just painting. In a 120px row, scaling the fixed 20px child by 2 changes the
split from `[20, 100]` to `[40, 80]`. A 10-device-pixel click offset becomes 5
local units after the doubled box is submitted, not when the edit is accepted.
No changes to renderer defaults, EMRG v10, or the sibling demo are required.

## What changed

- `tree/scale.rs` resolves authored values in device units, including main's
  different flat-root and nested rounding. Fonts scale at the declaration;
  descendants inherit its resolved size. At this checkpoint undeclared text
  still used 16 device pixels, as on main. The [follow-up](#default-font-size)
  changes that fallback only. `Transform.scale/1` is separate and not ported here.
- Admission stages new scale/font inputs and strike-count changes for the final
  parentage in `tree/wire/batch/domains.rs`. Closed interiors stop at equal
  inputs unless independently edited. The context pass compares against the
  last computed values, then layout and paint read the owner's scale. No pass
  runs during mutation. Actual text sites determine strike demand: the limit
  of 64 *declared* sizes is not a limit on effective sizes. Live tables, cold
  scratch and overlap are charged before commit; at most eight unused strikes
  are kept warm, evictable under the 64 MiB metric-table budget.
- `session/wayland/geometry.rs` binds fractional scale with viewporter, or uses
  core integer buffer scale. Layout takes the nominal factor. A submitted
  mapping keeps the logical size and rounded buffer extent; seat positions
  use their ratio separately on each axis. Shm attach, EGL swap and Vulkan
  present use the same mapping. A preference alone does not install anything.

## Evidence and limits

The old matched NIF rejected `unsupported attribute 78 in first slice`. Each
revision used its own producer. On 16 scenes (180×170), patched geometry matched
cold on all 16. On the 14 with display scale 1, whole ordinary binary viewport
frames matched cold mounts and export on all 14. Against pinned main, **13/16
geometry** and **12/16 whole RGBA** comparisons matched, including root-local
vs output scale, nested fonts, fractional row clips and text-bearing wrap.
A 25-pixel fringe at the last line of the latter exposed a missing WrappedRow
host clip; sharing Row's clip fixed it without changing sibling layout.

The exceptions are not hidden: `wrap`, its unscaled control and `round` reserve
actual line heights for empty-content boxes; main overlaps lines. The second
line in `wrap` starts at 29.375, not 4.375. At this checkpoint `local_shrink`
matched geometry but left 15 bottom-edge pixels outside a Column's device clip.
The column clipping follow-up below fixes that difference.
The **pre-scale retained NIF with zero tag-78 records** reproduced these outputs
byte-for-byte using equivalent authored dimensions, as did the new one; the
same unscaled controls reproduced main's outputs on main. This is scale support,
not a general change to Column/El overflow or a claim of full renderer parity.

- Ordinary viewport tests cover reuse at identical pixels, pending vs installed
  callbacks, inclusive fractional element edges, misses, child precedence and a
  press spanning a keyed move to a differently scaled lane. The native failure
  control keeps the preceding hits on a failed handoff. A localized chip edit
  moves a following sibling without remeasuring it. `1 → 2 → 1` before refresh
  can run no passes. A scale-only edit above 10,000 children is one 50-byte
  record; the declaration walk and affected context checks are not constant-time.
- Zero/negative/NaN/infinite factors, product overflow, cold-strike exhaustion
  and injected reserve failure preserve pending declarations. 69 effective sizes
  from one authored size were admitted. The measured warmed scale edits from
  wire through draw inputs, and the first compute after cold strike preparation,
  used **[0, 0, 0]** alloc/realloc/free calls in the Linux/glibc native harness.
  Cold admission, actual drawing and BEAM delivery are not allocation-free
  claims. Positive controls Rust `[1,1,1]`, Skia `[1,0,1]`; isolated initial
  text reservation `[33888,3589,33671]`, one-over text `[0,6,0]` and topology
  `[0,62,0]`. No matched timing.
- A private copy of `../wayland_cards` ran via `iex -S mix` on private Sway/shm.
  Root `scale(1.25)` with an explicit font, then only a scaled lane: compositor
  captures and actual `wl_seat` clicks showed the card moving, wrapping chips,
  Coral selection and normal close with IEx alive. At `1 → 1.5 → 2 → 1`,
  `301×202` at 1.5 produced **452×303**. Surface x=166 reached raw x=249.27574
  (not 249) and local x=8.41373 on the scaled right card. A second compositor
  withheld only the fractional global; `1 → 2 → 1` used integer buffer scale
  and passed the same application checks. No screenshot NIF or `send_input/2`
  was used for window acceptance. Both private compositors were stopped.
- Release Cargo tests and strict Clippy (including tests): headless **48**, shm
  **51**, GL **52**, Vulkan without GL **76**, both **76**, matched default **52**.
  Focused Mix **25 passed**; full Mix **646/740 passed**, the **same 94 failure
  names and 21 exclusions** as before. Standalone headless test, formatting,
  forced compilation, strict Credo and dev Dialyzer passed. Hardware fractional
  presentation, a forced real GPU acquire stall, and the opt-in Vulkan sync
  validation gate were **not run**; the CPU compositor and Cargo tests are not
  substitutes for them.

Private evidence: `/tmp/emerge-scale-root` points to `scale-yqejxK` under
`/workspace/emerge-investigation-artifacts`. `results/` contains main, pre-cut,
matched, unscaled controls, the failure list and source/binary manifests. See
`logs/{delivered,seal-wl,seal-integer,seal-test-*,seal-suite,finish-lib}`.
Mapped matched NIF `922c91b9ad989dcba0fa45a141aaf463aeec01d7e0080a8a3ad9518c158bca93`;
sibling source build `9ac6b1ddc93db9ba00d6f58f9e82c0691cf6e9347a6bfd6bc8d6ea71c8aacd75`.
Initial failed probes (missing private seat guard, guessed wrap y, mismatched
artifact, racing a forced compile, an app state replacement dropping viewport
metadata) remain in the logs. Corrected probes use the actual mapped artifact,
visible chip bands and ordinary state updates, not weaker assertions.

## Default font size

The retained renderer now treats an omitted font size as **16 logical pixels
at the text site**, rather than main's 16 device pixels. An explicit size still
resolves where declared and is inherited without multiplying it again. No wire,
renderer or demo change is needed to opt in. In the 480×240 cards fixture with
root `scale(1.5)` at display scale 1, the ordinary headless frame without a size
now matches the explicit 24-device-pixel frame byte-for-byte. The unscaled
frame is unchanged. Tests cover nested and display scales, adding/removing a
size declaration, and rejecting an overflowing fallback only where it is used.

Linux/glibc allocator checks still cover layout through prepared paint and hit
data, **not** drawing a new pixel buffer. With both an explicit size and the
default, the first computation after admitting a new font size and subsequent
warmed zoom edits (`1 → 2 → 1`) each measured **[0, 0, 0]** allocations,
reallocations and frees. Cold font preparation and growth can allocate. A
separate before-scale vs now diagnostic changed a 100×40 red box to blue behind
text in a 180×170 CPU frame. Three fresh processes per revision gave:

| Calling-thread heap calls (alloc/realloc/free) | Before scale | Now |
| --- | --- | --- |
| Fitting wire edit through layout and paint-input comparison | 0 / 0 / 0 | 0 / 0 / 0 |
| Rasterize and install the changed pixels after comparison | 7 / 0 / 7 | 7 / 0 / 7 |
| Reuse the installed pixels | 0 / 0 / 0 | 0 / 0 / 0 |

These are counts for that scene, not universal draw costs, GPU/BEAM counts or
timings. All six release Cargo test/Clippy configurations passed. Full Mix:
647/741 passed, the same 94 failure names and 21 exclusions. Private evidence:
`/tmp/emerge-default-font-scale-current`, `logs/{test-*,diag-*,cards-*,mix-all}`.
The `diag-*` probes used separate targets; discard the first shared-target
`paint-*` attempt, which reused the older executable.

## Column clipping follow-up

Ordinary Column descendants now share Row's host clip for paint and hit
regions. Their layout sizes and the gap between wrapped lines are unchanged.
`local_shrink` now matches the pinned main geometry and RGBA. In a 240×240
Wayland cards window at root layout scale 1.5 (display scale 1), exactly the
648 pixels in the lane gap changed from the previous retained frame; no chip
pixels changed. A real seat click there reached the containing row, not the
card. In the unmodified demo at 200 logical pixels wide the gap stayed dark:
clicking it left the card in place, and clicks inside moved it both ways.

Regression tests cover both clip axes, a fractional edge, shrinking and
widening the host, and a refused submission retaining the old clip. Evidence:
`/tmp/emerge-column-clip-current`, `logs/{compositor-delta,seat-candidate,demo-unit}`.
