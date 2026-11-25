# What isolating the live tree costs

**The benefit is progress under a stall, not faster unstalled edits.** With binary delivery held for 100 ms, the previous renderer accepted **0 of 32** subsequent edits before release; the new renderer accepted **32 of 32**, in each of three runs. Without a hold, median acceptance for one leaf among 10,000 children rose from **2.48 to 8.02 µs**. Median time from the same pre-encoded edit to the receiver rose from **34.28 to 42.68 µs**. Do not infer smoother display from either result.

## Which state won?

- The edit is applied once against the last **accepted** revision. The provisioner checks the declaration and reserves storage; NIF continuations write the private directory. A full journal refuses before inspecting the wire. No actor later reapplies a queued binary. [Admission](../../../native/emerge_skia/src/session/admission.rs) · [growth, refusal and sharing tests](../../../native/emerge_skia/src/tree/tests/publication.rs).
- The tree actor folds a finite prefix, then computes. The draw worker takes the newest eligible **unstarted commands**; at an opportunity the presenter takes the newest eligible **completed image**. In the gated 10k-tree burst, 32 revisions reached the command selector: **29 skipped before draw, 3 drawn, 1 completed but superseded, 2 installed**. The installed pixel stayed `1` while submission was closed. Releasing it installed revisions 18 then 33. A separate test held C *unfinished*: B still handed off. [Selector](../../../native/emerge_skia/src/session/presentation.rs) · [stage tests](../../../test/emerge_skia/stage_isolation_test.exs).
- Handoff installs that bundle, not the latest declaration. The private Wayland seat checked actual pixels and clicks after rejection, delayed release and resize. With a successful GPU handoff's report delayed across configure, the green frame installed its **101×75** mapping, and only the blue successor installed **152×113** at 1.5. Compare installed buffer/logical extent ratios, not just nominal scale. A definitely unsubmitted claim may retry; an uncertain handoff stops the output. Neither a successful handoff nor a frame callback proves physical scanout.

## Unstalled costs

`702e2653` → cutover, same host. **Microseconds, p50 / p95 / p99 / max** (nearest rank, pooled samples). “Accepted” ends when the native wrapper returns; “received” ends when the caller observes the matching 64×32 binary frame, possibly already in its mailbox. Neither includes EMRG encoding. Callback and resize rows have their own boundaries.

| Operation and boundary | Previous | Isolated |
| --- | ---: | ---: |
| Cold 10,001-node upload, accepted | 7,295 / 8,626 / 9,247 / 9,255 | 10,365 / 13,964 / 14,395 / 14,786 |
| Cold upload, received | 12,844 / 14,545 / 15,039 / 15,466 | 20,993 / 25,120 / 25,896 / 25,965 |
| 69-byte leaf edit, accepted | 2.48 / 3.27 / 7.38 / 168.28 | 8.02 / 11.65 / 62.70 / 82.25 |
| Leaf edit, received | 34.28 / 58.39 / 399.20 / 426.25 | 42.68 / 50.92 / 111.75 / 115.85 |
| Raw press + release → numeric-ID click received | 2.40 / 3.72 / 9.66 / 54.38 | 11.21 / 14.71 / 22.51 / 78.31 |
| Zero-wire route edit, accepted | 1.93 / 2.64 / 6.45 / 8.96 | 6.43 / 7.99 / 10.20 / 15.40 |
| Zero-wire route edit, received | 5.37 / 9.12 / 18.23 / 21.97 | 12.83 / 17.15 / 26.75 / 30.28 |
| 10,000-child reorder, accepted | 3,914 / 4,483 / 4,758 / 4,942 | 4,987 / 5,346 / 5,539 / 5,697 |
| Reorder, received | 5,221 / 5,882 / 6,106 / 6,287 | 8,083 / 8,591 / 8,877 / 8,980 |
| Offered root width, accepted | 7.02 / 20.41 / 30.78 / 147.04 | 34.89 / 65.84 / 80.63 / 266.03 |
| Offered root width, received | 686 / 990 / 1,154 / 1,278 | 1,643 / 2,040 / 2,099 / 2,141 |
| Inline 4 MiB dummy source, accepted | 2,723 / 3,133 / 3,433 / 3,450 | 3,434 / 3,873 / 3,934 / 4,259 |
| Dummy source, first frame | 2,739 / 3,164 / 3,471 / 3,478 | 3,502 / 3,929 / 4,041 / 4,389 |
| Inline 4,673,815-byte PNG, accepted | 2,599 / 3,008 / 3,167 / 3,358 | 4,009 / 4,516 / 4,607 / 4,760 |
| PNG, first frame containing its fitted red corner | 5,620 / 6,274 / 6,540 / 6,830 | 7,072 / 7,770 / 8,282 / 8,324 |
| Real viewport callback-only edit: application message → frame | 25.97 / 38.98 / 52.02 / 57.74 | 41.54 / 59.43 / 76.50 / 88.79 |
| Same viewport: press + release → `handle_info/2` with that callback and local point | 4.51 / 7.18 / 8.32 / 10.30 | 14.41 / 21.08 / 25.24 / 38.89 |
| Private shm surface: resize request → observed `:resized` | 587 / 838 / 895 / 943 | 597 / 644 / 847 / 872 |
| Same resize → verified capture of the new installed extent | 601 / 864 / 1,657 / 1,682 | 642 / 691 / 885 / 915 |

The zero-wire native probe changes the route epoch without building a viewport map. The separate one-chip viewport uses the real producer, relay and callback, and checks the message on every edit. The root-width edit is **not** a Wayland resize. The shm case alternates **129×95 / 128×96** through Sway IPC: the second mark includes the request, configure, handoff and capture/poll, not just `wl_surface.commit`. The dummy media case tests ownership/validation, not decode throughput.

**Repeatability:** Four paired blocks, baseline first in blocks 1 and 4. On each metric, take the larger of the two arms' max−min ranges of block medians. Call the median difference resolved only if *all four* paired differences exceed that tolerance in the same direction. Example tolerances: leaf acceptance **0.08 µs**, leaf receipt **2.89 µs**, viewport click **0.52 µs**, resize → installed capture **25.90 µs**. Offered-width acceptance, dummy media and resize → `:resized` did **not** clear their tolerances. The remaining medians in the table were longer on the isolated path. These are descriptive repeatability checks, not confidence intervals or tail guarantees.

**Separate Wayland control, stats enabled.** Four paired blocks × 40 resizes per arm/API. Each resize produced exactly one counted handoff. The first two clocks start before the Sway request: one stops at receipt of `:resized`, the other at a red capture of the new extent. The latter includes explicit capture (GPU readback on GL/Vulkan) and a 1 ms retry sleep if the old size is still installed. The third clock, `present_submit`, measures part of the native handoff, not either of those end-to-end intervals.

| Private seat, µs (p50 / p95 / p99 / max) | Previous | Isolated |
| --- | ---: | ---: |
| shm request → `:resized` | 607 / 1,038 / 1,357 / 1,700 | 595 / 677 / 804 / 1,079 |
| GL request → `:resized` | 573 / 756 / 820 / 830 | 743 / 905 / 965 / 1,036 |
| Vulkan request → `:resized` | 578 / 818 / 852 / 863 | 673 / 903 / 946 / 1,039 |
| shm request → installed capture | 623 / 1,077 / 1,741 / 2,165 | 637 / 721 / 845 / 1,122 |
| GL request → installed capture | 814 / 991 / 1,024 / 1,030 | 2,896 / 2,960 / 2,992 / 3,068 |
| Vulkan request → installed capture | 1,071 / 1,279 / 1,443 / 1,642 | 2,913 / 3,939 / 4,011 / 4,011 |
| shm `present_submit` | 51 / 92 / 143 / 175 | 6 / 7 / 8 / 11 |
| GL `present_submit` | 140 / 191 / 205 / 211 | 220 / 240 / 270 / 777 |
| Vulkan `present_submit` | 59 / 71 / 80 / 84 | 561 / 665 / 1,091 / 1,399 |

All three `:resized` comparisons were unresolved. The comparable request → installed capture median was longer on GL and Vulkan (tolerances **43.12 / 28.40 µs**), unresolved on shm. Shm and GL used the ordinary Wayland/OpenGL build: shm selected CPU and GL selected radeonsi on the RX 7900 XTX. Both Vulkan-only builds selected hardware RADV on the same render node (226:128).

**The native clock changed scope.** Old shm timed `Prepared::submit`, including filling the slot and installing the raster; new shm times mapping/damage/attach/commit/flush of an already filled slot. Old GL/Vulkan started after acquire and recording the final blit, and timed swap/present, flush and reacquiring `Owner.live`. New GL times the winner's resize, blit, synchronized flush, swap and connection flush; new Vulkan includes acquire/recreation as needed, the winner's transfer/present and connection flush. Neither new GPU timer includes drawing the source, waiting for the job to start or dispatching its success report. The shorter shm or longer GPU submit spans do not isolate the cost of the driver call. None measures scanout.

## Where the work moved

Separate instrumented release NIFs, not used for the preceding wall times. For 100 leaf edits, the candidate's per-stage **µs** below give histogram-bin upper bounds for p50/p95/p99, then the exact observed max. Allocation totals count Rust requests on the timed thread, not BEAM, Skia, driver, frees or RSS. Stage medians are not additive.

| Leaf boundary | p50 / p95 / p99 | Max | Rust requests / requested bytes (100 edits) |
| --- | ---: | ---: | ---: |
| Begin ticket | 1.343 / 1.727 / 2.303 | 5.220 | 100 / 304,000 |
| Feed the wire | 0.191 / 0.247 / 0.415 | 1.850 | 200 / 9,400 |
| Request proof | 0.083 / 0.091 / 0.183 | 0.850 | 0 / 0 |
| Proof worker | 1.343 / 2.303 / 6.655 | 12.600 | 616 / 71,088 |
| Apply directory slice and publish | 1.343 / 1.919 / 4.607 | 4.600 | 0 / 0 |
| Fold publication | 0.287 / 0.351 / 0.863 | 8.020 | 0 / 0 |
| Layout | 12.799 / 13.823 / 28.671 | 29.809 | 0 / 0 |
| Collect | 11.775 / 12.287 / 19.455 | 19.140 | 0 / 0 |
| Draw and prepare binary | 1.343 / 6.143 / 7.167 | 7.080 | 200 / 823,200 |
| Deliver binary and install | 1.215 / 1.919 / 9.215 | 10.600 | 1,700 / 65,600 |

- **Sparse vs wide:** Each leaf edit fed 69 wire bytes, wrote one record, replaced 16 directory pages and copied **zero child-order bytes**. Fold, layout, collection and the apply slice made no Rust allocation requests in this case; the whole transaction was not allocation-free (see begin, feed and proof above). Reorder fed 80,039 wire bytes and wrote 80,000 bytes of child keys. A route-only publication wrote no directory pages or child keys. Drawing/hit buffers cross owners as handles; flat collection on a changed side and copying/converting pixels for the output are separate costs.
- **Work per entry:** Old cold upload's single source-NIF body reached **7.55 ms**. Candidate cold proof reached **10.99 ms on the provisioner**, while the max cold feed/apply bodies were **28.92/11.72 µs**. Four one-bit-offset **64 MiB** uploads used 65,536 feed calls total; their p99 was ≤5.12 µs and max **207.82 µs**; proof reached 25.62 ms off-scheduler. A **25-byte clear** removed 10,001 nodes in **313 apply calls**, not one long NIF; max across four clears **9.62 µs**. This bounds work on these inputs, not worst-case scheduler wall time. Timer, unrelated dirty-CPU and dirty-I/O calls also completed in deterministic held-stage tests with one scheduler in each dirty pool.
- **Under the 100 ms hold:** pooled acceptance p50/p95/p99/max for 96 small edits was **5.94/19.26/101,126.51/101,126.51 µs** before and **9.47/19.76/31.16/31.16 µs** after. The first baseline call was blocked in each run. A probe held the old renderer before binary send, or the new renderer after claiming a completed image; the receiving BEAM process was not flow control. Headless does not bound its recipient's mailbox.
- **Charged high water** (document / workspace / inline media, bytes): wide tree **12,052,927 / 81,993,728 / 0**; PNG **4,682,934 / 83,968 / 4,674,131**; maximal wire **67,117,860 / 83,968 / 67,109,077**. These maxima are per-account, not necessarily simultaneous. The workspace credit follows sealed readers across growth. This is not total memory: e.g. a 64×32 RGBA target carries 8,192 pixel bytes before binary delivery. GPU targets, Skia/driver caches and receiver mailbox retention are not measured by these counters. [Ledger](../../../native/emerge_skia/src/tree/storage.rs) · [allocation tests](../../../native/emerge_skia/src/tree/tests/allocations.rs).

## Motion and opportunities

An internal two-second 0→48 px track sampled absolute time at unchanged declaration revision 1. Three runs per cap/treatment, no per-frame logging. All reached the terminal pixel, checked installed capture, and disarmed when idle. A 100 ms submission hold did not enqueue the missed steps. These are **binary handoff measurements**, not vblank timestamps. A repeated rounded pixel need not mean a repeated sample time.

| Cap / hold | Handoff interval p50 / p95 / p99 upper bounds¹ (ms) | Longest interval (ms) | Largest late age / early lead² (ms) | Estimated periods missed per run³ |
| --- | ---: | ---: | ---: | ---: |
| 60 / none | 16.78 / 17.83 / 17.83 | 17.22 | 0.554 / 0.050 | 0 |
| 60 / 100 ms | 16.78 / 17.83 / 17.83 | 101.31 | 84.647 / 16.588 | 5 |
| 120 / none | 8.92 / 8.92 / 9.44 | 8.97 | 0.637 / 0.038 | 0 |
| 120 / 100 ms | 8.92 / 8.92 / 9.44 | 101.12 | 92.785 / 8.256 | 11 |

¹ Largest per-run bin upper bound, not a pooled quantile. ² Handoff minus the bundle's sample time; early counts use the opposite sign. ³ Sum of `max(0, round(gap / period) − 1)`; estimates, not feedback. Holding submission can hand off an old eligible sample, then immediately a newer one. The diagnostic *before* correcting headless prediction saw ~33.55/16.78 ms median bins at 60/120. It included trace output and is not a paired speed comparison. The [clock regression](../../../native/emerge_skia/src/session/timing.rs) checks the cause independently: work may start **before** the draw deadline, and headless has no refresh grid to round onto. Public retained `Animation` and real KMS are not part of this change.

## Qualification and reproduction

- Runtime/test input manifest SHA-256 `e8eee86df4ec7017a003e6858557bb573dba48622c17029feb078c6c17fd19a2` matches both the measured ordinary candidate and the qualified worktree. Private snapshot `clock2` is `efc5e92124041a69eeb9484d978baa4c61c78e16b6d6017a4512e983ac93f934` including its build config; neither hash is a Git commit. Cost controls use `git archive` of `702e2653b23d89b1a02ad83794de3b614e0bb62b` and matching private configs. Each arm checks source manifest, mapped NIF bytes/inode and loaded BEAM paths/content, not just module names.
- Locked native release tests: CPU **227**, default GL **231**, Vulkan **255**, Rust 1.91 default **231**, embedded CPU **227**, shm-only **230**. Strict Clippy on the live profiles, Mix/Cargo format, warning-clean builds, strict Credo and Dialyzer. The focused headless run passed **54**. On the private AMD seat **9 shm / 10 GL / 12 Vulkan** passed, including Vulkan hardware synchronization. Ordinary (no stage probes) CPU/embedded, shm/GL and Vulkan also passed the full-buffer SVG, warm-remount and detail controls. No claim for Apple hardware, real retained DRM/KMS, migration/device loss or physical scanout.
- Ordinary `mix test --seed 17` on `clock2` exited **2**: **840/898 passed** (15 doctests and 825 tests), **58 failed**, **21 excluded**. Both name sets matched the pre-cutover baseline. A fresh documentation-only checkout had one additional 100 ms raw-release timeout in `LinearEvenSpacingTest` (839/898); that case then passed six times alone, and an unchanged full rerun again matched both baseline lists. The timeout's cause is not established. This is not an all-green CI or package release claim. The last full package check stopped at `counter-basic` (`Event.on_press/1`, unsupported attribute 61); this cutover did not port that attribute.
- Private reproduction: `/workspace/.emerge-owner-cutover-4Um5UH/owner-release/HOWTO.md`. Raw runs and per-block medians in `cost/results/`, NIF body/allocation series in `stages.csv`, signed pose distributions in `motion{1,2,3}.log`. Use the [performance lock](../../../scripts/performance-lock.sh) shared for correctness, exclusive for timings. Timed VMs: Ryzen 9 7950X, CPUs 8–15, `+S 4:4 +SDcpu 2:2 +SDio 1`. The unstalled headless fixtures set no draw cap and zero tree headroom; the resize fixtures used renderer defaults for their selected Wayland API. Per arm: 400 samples per warm native case, 100 cold or media, 160 per surface resize, 400 per viewport case after 10 warmups per VM. No samples dropped from completed blocks. Instrumentation and virtual-seat tooling are not release features.
