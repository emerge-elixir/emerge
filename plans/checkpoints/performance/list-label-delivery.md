# Why one label can take longer to deliver

**Finding:** It is not a layout regression. On the **960×640 delivery workload itself**, retained got from native patch entry to prepared draw/hit data in **0.038 ms**, versus **0.891 ms** from entry to the scene queue on `main`. With the BEAM producer bypassed, the same edit reached the receiving process **47% sooner**. The stage that can spend that saving is **BEAM reconciliation and garbage collection before the patch is submitted**.

The historical [shared-surface baseline](shared-surface-baseline.md) recorded **3.19→3.49 ms** (+12.8%, 95% interval +1.3% to +25.5%). Replaying that public benchmark **without changes** gave **3.307→3.317 ms**, paired **+1.3% [−1.7%, +4.4%]**. The slowdown was not reliably reproduced; neither the exact historical cost nor its cause can be recovered from the old aggregate. This is an explanation of the observed trade-off, not a claim that every label edit is slower.

## Where the time goes

`main` `aef5e42fc789c396505f85c34810b2ca9c008586` → retained `ffa22073c663f61b7dda59c11f0ca8af67422abc`. One change and its undo in row 5 of a 500-keyed-row list, 1,002 mounted nodes and 250 listeners. All times below are milliseconds.

| Measurement | main | Retained | Paired difference [95%] |
| --- | ---: | ---: | ---: |
| Public `patch_tree/3` → frame, unchanged benchmark | 3.307 | 3.317 | +1.3% [−1.7%, +4.4%] |
| **Pre-encoded patch → received frame**, no producer | 1.901 | 1.056 | −47.1% [−51.8%, −42.0%] |
| `TreeRenderer.patch_tree_runtime/3` → received frame | 2.494 | 1.891 | −25.9% [−33.5%, −17.4%] |
| **Real viewport:** application message → received frame | 2.747 | 2.089 | −24.6% [−26.5%, −22.7%] |

The viewport's `handle_info/2` selects a prebuilt declaration and requests `Viewport.rerender/1`; normal render, diff and native submission follow. Its [renderer path](../../../lib/emerge_skia/tree_renderer.ex) does **not** request a fully assigned return tree. The public helper does. The two API boundaries are not interchangeable.

Separately, an isolated split put a clock before `Engine.diff_state_update/2`, at patch readiness, at transport return and at the expected frame on **each edit**. Arithmetic means across eight paired VMs (not sums of separately sampled medians):

| Same-call span | main | Retained | Paired retained − main [95%], ms |
| --- | ---: | ---: | ---: |
| Assigned-tree producer | 1.008 | 2.120 | **+1.112 [+0.673, +1.551]** |
| Transport call | 0.801 | 0.837 | +0.036 [−0.085, +0.157] |
| Transport return → receipt | 1.177 | 0.219 | −0.958 [−1.094, −0.821] |
| **Patch ready → receipt** (previous two rows) | 1.978 | 1.056 | **−0.922 [−1.057, −0.786]** |
| Total | 2.986 | 3.176 | +0.190 [−0.317, +0.698] |

The receiver and workers share a pinned CPU: a frame can already be in the mailbox when the caller resumes. Neither “transport call” nor “wait after return” is a pure rendering cost. The stronger check is bypassing only the producer and replaying each revision's own bytes. The forward/undo patches are 87/79 bytes on `main`, 113/105 bytes on retained. In both cases the receiver gets the full 2,457,600-byte RGBA frame.

## Why the producer is sensitive to the caller heap

- A sparse **wire patch is not a sparse BEAM walk**. Both reconcilers visit the list. On retained, OTP `tprof` counted **1,002** calls each to [`Reconcile.enter/5`, `claim/3` and `close/7`](../../../lib/emerge/engine/reconcile.ex) per edit. The walk claims IDs and explicit keys, folds old and new child lists, maintains occurrence/site metadata, and in assigned mode constructs the returned `Element` tree. It reuses stable VNodes; it does **not** resend the 499 untouched rows.
- Per assigned edit, the traced engine functions allocated **177,623→278,023 BEAM heap words**. On retained, about **75,774** were attributed to `claim/3` and **66,427** to `close/7`. These are allocation counts, **not function timings**, and not just the cost of returning the assigned tree: even binary-only, the counts were **139,472→263,998**. The resulting assigned `DiffState` had **54,792→138,294** reachable words (`:erts_debug.size/1`, counting shared terms once); the binary-only state had **41,759→102,802**. Neither metric is RSS or native heap usage.
- In a separate matched run with process GC timestamps, mean assigned-producer time was **1.209→2.018 ms**. Of that, **0.284→0.891 ms** overlapped a collection in the caller. Thus **0.607 ms** [0.493, 0.722] of the **0.809 ms** excess was in GC in this run. The remaining **0.201 ms** [0.167, 0.236] was outside GC; it is not a profile of any one function.
- **Counterfactual:** give just the calling process a larger minimum heap on both arms, leaving declarations, producer, patch bytes and rendered pixels unchanged. At 2,072,833 words (~15.8 MiB), **no collection overlapped the producer span**; its means were **0.875→0.899 ms**, paired difference **+0.023 ms** [0.010, 0.036]. Collections still occurred elsewhere. This distinguishes GC sensitivity from an intrinsically expensive native text edit. It is a diagnostic, **not** a recommendation to reserve 16 MiB per viewport.

**Does changing the heap matter without any instrumentation?** Yes. A final control ran the list portion of the original benchmark, keeping `latency.exs` byte-for-byte identical and tracing **off**. The only change between treatments was the minimum heap of the calling process, set before starting its renderer. At the usual minimum the public frame took **3.369→3.397 ms** (`main`→retained, paired +1.9% [−0.9%, +4.8%]); at the larger minimum it took **3.211→1.942 ms** (−38.3% [−41.9%, −34.5%]). Within retained that is **−1.456 ms** [−1.559, −1.354], versus −0.177 ms [−0.414, +0.060] within `main`. The change in the retained-minus-main gap was **−1.279 ms** [−1.509, −1.050]. Eight balanced blocks of four fresh VMs; interfaces, patching, frame checks and renderer binaries unchanged.

**Why this is not a retrospective attribution.** Inserting the split into a copy of the *full* benchmark instead gave producer means **1.198→1.059 ms** and frame means **3.488→2.086 ms**. An independent OTP call/GC trace left the original caller code intact, joining each label edit by its actual alternating patch lengths. There, producer means were **1.408→1.114 ms**, including **0.357→0.148 ms** in GC; patch ready → receipt was still faster (**2.210→1.015 ms**). Hook-to-hook totals and the caller's own timer differed by at most 1.5 µs on those 512 measured frames, but the *traced workload* plainly did not behave like the untraced one. The two profiled producer modes both allocated more on retained; wall time depends on when collections fall. The native delivery advantage survives the different observers.

## Native work at delivery size

Private release NIFs with timestamps at existing boundaries, **960×640**, ordinary public producer and real binary sink. Mean milliseconds; the native marks include scheduling, not just CPU instructions. The `main` tree publishes to a render queue; retained settles in the display worker. These are comparable milestones, not identical call graphs.

| Native span | main | Retained |
| --- | ---: | ---: |
| Patch handler entry → outputs ready / scene queued | **0.891** | **0.038** |
| Raster draw / command replay | 0.434 | 0.737 |
| After draw → pixel data ready | 0.567 | 0.001 |
| Write pixels into outgoing BEAM binary | 0.055 | 0.044 |

Retained's draw span **was** slower (+0.304 ms [0.187, 0.421]); that is not the whole raster pipeline. `main` extracts the surface into an owned pixel vector; retained [draws into its candidate vector](../../../native/emerge_skia/src/tree/paint.rs). Both still write the output binary. The draw measurement does not separate glyphs, clears or first-touch costs. Taken with the **unmodified-NIF pre-encoded result**, neither redraw nor the binary copy accounts for a net retained delivery deficit after patch readiness. No subtraction from the earlier **960×4000** layout benchmark, no individual layout-pass or GPU claim.

## Reproduction and limits

- Eight independent paired blocks per timing experiment, alternating arm order (balanced four-condition order for the untraced heap control); process median for public/pre-encoded/viewport comparisons, ratio interval on paired logs. For additive spans, arithmetic means and paired *absolute* differences. Approximate unadjusted 95% Student-t intervals, df=7. Twelve warm-ups and 80 alternating edits for focused cases; original workload preserved its full order and six warm-ups/32 label edits. No discarded timing pairs. No intentional frame pacing.
- Same CPU-raster declarations, matched BEAMs and mapped release NIFs as the baseline, Ryzen 9 7950X CPU 8, exclusive [performance lock](../../../scripts/performance-lock.sh), one scheduler and one Rayon worker. Receipt fenced by the changed pixel, full `VideoInterop.Frame` validated and compared against fresh expected pixels **after** the clock. This is not scanout or user-input latency.
- Evidence: `/workspace/emerge-investigation-artifacts/list-label-delivery-ZxYb6t/` — `HOWTO.md`, `tools/`, `raw/`, `analysis/`, `probe2/`, per-suite `*-inputs.sha256` and `evidence.sha256`. Input manifest SHA-256: `4235a360bad1fa867796ead87c58d793dde4b882f2ab7112256bad45e8a2ec7e` (unchanged replay), `b645318b779b6e88aaf9876585fbc7fd92b7961428ec79f43a374ed15946c311` (untraced heap control). Native probes and GC experiments are diagnostic, not replacements for the unchanged replay. Probe v1 let a later `main` scene overwrite the final frame's queue stamp; it is preserved but **not used**. V2 sealed each record at send completion and checked patch sizes, order, monotonic marks and zero pre-completion duplicates for all 1,472 warm-up/measured edits.

**Next target:** reduce temporary allocations in the existing occurrence-claim and child-fold path; benchmark both assigned and binary-only updates and keep global key validation, sparse bytes and route semantics intact. No fix, heap-policy change, port selection or production code change was made.
