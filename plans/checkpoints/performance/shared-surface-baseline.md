# First shared-surface CPU benchmark: retained vs `main`

**Comparison.** Retained `ffa22073c663f61b7dda59c11f0ca8af67422abc` vs local `main` / `origin/main` `aef5e42fc789c396505f85c34810b2ca9c008586`. This measures the common, implemented API, not either renderer's entire workload. No implementation change or port selection.

**Result:** Retained used less time in **14/14 native compute cases** and delivered the expected raster frame sooner in **8/9 public cases**. The exception was editing one list label: native patch + refresh fell from **1,005 to 29 µs**, but patch-to-frame latency rose from **3.19 to 3.49 ms** (paired **+12.8%**, 95% interval **+1.3% to +25.5%**). In contrast, rewrapping the card grid fell from **4.76 to 2.30 ms** at the public boundary.

The clearest regression is on the **BEAM producer: 12/12 cases took longer**. Changing one inherited Font declaration cost **0.54→2.42 ms** to produce a 70-byte retained patch, while the retained native side applied and settled it in **54 µs**. This baseline identifies where to look, not which function caused the regression. The [one-label follow-up](list-label-delivery.md) separates producer/GC from post-patch delivery and reports the result of replaying the original benchmark unchanged.

## What ran

Two workloads adapted from the refresh investigation's [scenario generator](../../../bench/support/scenarios.exs):

| Workload | Same declarations on both arms | Alternating A→B→A edits |
| --- | --- | --- |
| Text list | 500 keyed rows; 1,002 nodes; single-line Inter 16, alternating solid backgrounds and 250 click listeners | Change row 5's text; add/remove its listener; reverse all 500 keys |
| Card grid | 100 keyed cards; 1,302 nodes; 100 listeners. WrappedRow, padded Columns, Rows, centered single-line labels, fixed gaps and weighted fills | One background; inherited center↔right on 100 labels; fixed↔even spacing in one Row; wrap width 940↔936 (six↔five cards per line); one pair of fill weights 1:3↔3:1 |

The investigation's scenarios are a workload model, not a binary format common to both revisions. As an admission control, `main` accepted its 11 checked-in full benchmark fixtures; retained refused all 11 of its own on `unsupported attribute 13 in first slice`. None of those refusals was timed. Each arm generated its own fixtures: `main` EMRG v9, retained v10. Full list: 81,486 vs 82,504 bytes; full grid: 55,804 vs 57,122. A one-card paint edit was 64 vs 90 bytes; a list reversal 4,011 vs 4,039. No timing below replays another arm's bytes or imports the old `5c71a59` timings. The 500 rows are mounted, not all on screen: about 158 intersect the tall viewport, 26 the delivery viewport.

**Correctness before timing.** At both 960×640 and 960×4000, all 20 complete fresh RGBA buffers and all 23,640 node rectangles (in declaration order, not by incidental ID) matched across revisions. Each visible edit and its undo matched a fresh public frame; every sparse layout and undo matched fresh geometry. Each native benchmark adapter separately reproduced 24 tall public buffers, including the event-only change. At `(5, 106.4375)` the prepared press target was absent → row 5 → absent; the BEAM registry went 250→251→250. This checks output and registry changes, not real seat input. Every timed public frame was compared with the complete expected buffer **after** stopping the clock.

The gate rejected a preliminary 36px-high card: its children naturally fit 34px, and the wrapped grid measured 642px tall on `main` vs 676px on retained. The timed card is 34px on **both** arms. No mismatching frame was included as a speed win. There is no `main` WrappedColumn; Nearby, paragraphs/TextColumn, rich paint, media, scrolling and animation are outside this comparison.

## Timing boundaries

- **Native compute, 960×4000:** optimized Rust, no BEAM, raster, rectangle enumeration or frame delivery. `mount` includes full-byte decode, provisioning, first layout/scene and hit preparation, and retirement. `cold_refresh` times the first computation on a fresh, already uploaded tree, excluding upload and drop. Both are process-warm, not fresh font-system startup. `clean` explicitly requests another refresh on an unchanged tree. Other cases decode and apply a real alternating patch and settle the required outputs. `main` reuses its clean registry and relayouts a patch only when required; retained settles and rotates its prepared draw/hit inputs without a pixel sink.
- **BEAM producer:** prebuilt declarations into `Engine.encode_full/2` or the binary-only `Engine.diff_state_update_binary/2`. The no-op compares equivalent declarations. This includes reconciliation/serialization and GC as incurred, not UI construction, transport, native work, or assignment of a public return tree on the binary-only path.
- **Public delivery, 960×640:** start the timer before `EmergeSkia.upload_tree/2` (fresh session already started) or `patch_tree/3` (settled session); stop on receipt of the expected `VideoInterop.Frame`. Includes the public producer, native work, raster, binary handoff and mailbox wait. A changed pixel identifies the expected state; mere acceptance is not completion. No target FPS cap. A listener-only edit need not emit new ink, so it has no delivery time. These three timing boundaries are **not additive**.

## Measured results

All 35 cases; smaller is better. The paired percentage need not equal the ratio of the two displayed medians.

### Native compute

| Case | main (µs) | Retained (µs) | Paired Δ time [95%] |
| --- | ---: | ---: | ---: |
| List: upload + first refresh + retire | 3,057.7 | 2,171.0 | −29.3% [−30.0%, −28.4%] |
| List: first refresh only | 2,235.2 | 935.5 | −58.1% [−58.4%, −57.9%] |
| List: clean refresh | 462.5 | 0.032 | −99.993% [−99.993%, −99.993%] |
| List: one label | 1,005.2 | 29.0 | −97.1% [−97.1%, −97.1%] |
| List: one listener | 614.3 | 2.88 | −99.5% [−99.5%, −99.5%] |
| List: reverse keys | 1,699.8 | 286.9 | −83.1% [−83.3%, −82.9%] |
| Grid: upload + first refresh + retire | 4,557.0 | 1,960.5 | −56.9% [−57.9%, −55.9%] |
| Grid: first refresh only | 3,397.8 | 672.5 | −80.1% [−80.5%, −79.7%] |
| Grid: clean refresh | 926.0 | 0.032 | −99.997% [−99.997%, −99.997%] |
| Grid: one background | 922.9 | 10.1 | −98.9% [−98.9%, −98.8%] |
| Grid: inherited font alignment | 2,275.5 | 53.5 | −97.7% [−97.7%, −97.6%] |
| Grid: even spacing | 1,345.7 | 10.7 | −99.2% [−99.2%, −99.2%] |
| Grid: rewrap | 1,597.0 | 131.9 | −91.6% [−91.9%, −91.4%] |
| Grid: weighted widths | 1,344.5 | 11.7 | −99.1% [−99.2%, −99.1%] |

The 32 ns result is the cost of settling **unchanged, already prepared** outputs; it does not draw a frame. Intervals that look like a point are rounded; full precision is in `summary.json`.

### BEAM producer only

| Case | main (µs) | Retained (µs) | Paired Δ time [95%] |
| --- | ---: | ---: | ---: |
| List: encode full | 1,559.4 | 3,769.3 | +142.3% [+141.0%, +143.6%] |
| List: no-op | 585.7 | 847.5 | +42.4% [+37.0%, +48.1%] |
| List: one label | 583.2 | 888.7 | +49.5% [+42.7%, +56.6%] |
| List: one listener | 589.8 | 891.1 | +48.7% [+43.3%, +54.3%] |
| List: reverse keys | 690.3 | 1,813.3 | +158.7% [+149.0%, +168.9%] |
| Grid: encode full | 1,822.3 | 4,172.0 | +129.9% [+126.4%, +133.4%] |
| Grid: no-op | 563.0 | 953.3 | +69.3% [+56.5%, +83.1%] |
| Grid: one background | 568.2 | 966.9 | +70.3% [+56.6%, +85.4%] |
| Grid: inherited font alignment | 538.7 | 2,420.1 | +342.8% [+321.7%, +365.0%] |
| Grid: even spacing | 536.9 | 987.8 | +82.1% [+72.4%, +92.5%] |
| Grid: rewrap | 526.6 | 975.9 | +82.6% [+74.4%, +91.1%] |
| Grid: weighted widths | 530.5 | 988.4 | +85.2% [+77.0%, +93.8%] |

### Public call to delivered raster frame

| Case | main (ms) | Retained (ms) | Paired Δ time [95%] |
| --- | ---: | ---: | ---: |
| List: first frame after upload | 12.3 | 11.7 | −5.2% [−7.1%, −3.2%] |
| List: one label | 3.19 | 3.49 | **+12.8% [+1.3%, +25.5%]** |
| List: reverse keys | 5.99 | 4.31 | −29.3% [−34.0%, −24.4%] |
| Grid: first frame after upload | 14.6 | 11.8 | −20.0% [−22.5%, −17.5%] |
| Grid: one background | 3.57 | 2.21 | −38.3% [−44.6%, −31.3%] |
| Grid: inherited font alignment | 5.63 | 4.68 | −16.0% [−20.6%, −11.1%] |
| Grid: even spacing | 4.45 | 2.23 | −50.4% [−51.6%, −49.1%] |
| Grid: rewrap | 4.76 | 2.30 | −51.9% [−54.0%, −49.8%] |
| Grid: weighted widths | 4.42 | 2.26 | −48.5% [−50.1%, −47.0%] |

## Method and replay

- Six independent paired blocks in alternating `main`/retained order, no discarded rounds. Exclusive [performance lock](../../../scripts/performance-lock.sh), CPU 8 on a Ryzen 9 7950X, `RAYON_NUM_THREADS=1`. BEAM `+S 1:1 +SDcpu 1 +SDio 1 +A 1`. Governor `powersave`, boost enabled; no host settings changed. Rust/Cargo 1.93.1, Elixir 1.20.2 / OTP 29. Native Criterion: release defaults + `bench-diagnostics`, arm-specific Cargo locks, same pinned Skia/font. Public: private, revision-matched BEAMs and mapped release NIF, `[wayland: [:opengl]]` build matrix, CPU raster exercised.
- Criterion: 0.5s warm-up, 1.5s measurement target, 30 samples, 5,000 resamples. Process estimate = slope for linear sampling, mean for flat. Public/producer = median per VM; 80 producer samples (12 warm-up), 16 first frames (4 warm-up), 32 changed frames (6 warm-up) per case/arm/block.
- Absolute times are medians of six process estimates. Δ is the geometric mean of six paired retained/main time ratios minus one; brackets are approximate, **unadjusted** 95% Student-t intervals on the log ratios (df=5). Negative is less time. A case whose interval crosses zero is inconclusive, not proof of equality. No grand speedup, FPS, GPU, Wayland presentation, memory, or input-to-light claim.
- Evidence and replay: `/workspace/emerge-investigation-artifacts/shared-surface-bench-Y7bFih/`. `HOWTO.md`, `tools/{scenes.exs,shared_surface.rs,main.rs,retained.rs,latency.exs,measure}`, `timed-inputs.sha256`, `raw/`, `observations.csv`, `summary.json`, full frames/geometry and `analysis/`. The measured-input manifest SHA-256 is `f04d6eec0f81a9cc7bee3bfe6280ef17fb0a71a57eb9c599c78c95bc53346f24`. All 24 measurement processes exited successfully: 168 Criterion estimates and 14,592 BEAM/public observations, reduced to 420 case/process estimates. Replay in a **copy**, not over the original results.
