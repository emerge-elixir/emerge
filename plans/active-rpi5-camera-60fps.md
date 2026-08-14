# RPi5 Camera stable-60 plan

Status: shared Phase 0 instrumentation implemented; hardware overhead/baseline capture pending.

## Goal

Improve the exact live Camera Focus-interaction scene from the measured 15.52 ms GPU
render elapsed / 50.6 FPS to:

- median GPU render elapsed <= 10.86 ms (30% reduction);
- p95 <= 11.67 ms and p99 < 16.67 ms;
- 59.8-60.2 presented FPS with no sustained missed-vblank sequence gaps;
- unchanged exact pixels, interaction behavior, ownership, and shutdown semantics.

The 30% target is the acceptance gate, not an estimate. Gains are measured on RPi5 and
are not assumed additive.

## Locked constraints

- Keep renderer caching. Cache-off measured 33.22 ms / 25.8 FPS active and
  28.50 ms / 32.2 FPS idle.
- Keep exactly nine semantic Camera layers, direct Video, exact clip/paint order,
  focus glow, changing text, native immediate thumb movement, 20 Hz publication,
  60 ms camera application, and exact final state agreement.
- No Camera node IDs or scene recognition in renderer policy.
- Keep full DMA-BUF ownership, acquire/retirement fencing, lease lifetime, DRM
  fallback, and complete on-demand screenshots.
- RPi5 live DRM is authoritative. GPU-complete desktop Criterion is only a guardrail.

## Quantitative baseline

| Window | FPS | GPU elapsed | Missed vblanks |
| --- | ---: | ---: | ---: |
| cache-on idle | 59.2 | 8.87 ms | 4 |
| cache-on Focus active | 50.6 | 15.52 ms | 47 |
| cache-on recovered | 60.0 | 10.01 ms | 0 |
| cache-off idle | 32.2 | 28.50 ms | 139 |
| cache-off Focus active | 25.8 | 33.22 ms | 170 |

The active build must remove at least 4.66 ms. If the 8.87 ms idle floor stays fixed,
70% of the 6.65 ms interaction increment must disappear.

The exact current Focus fixture reports about 33 payload candidates and 1.369 million
payload pixels. One bottom-panel border run alone occupies about 507k mostly-transparent
payload pixels. Cache granularity is therefore useful for invalidation but fragmented for
primary-surface composition.

## Direction decision

Use two isolated implementation tracks:

1. **Track A, preferred/main worktree: deletion and work removal.** Fix correctness,
   delete the provisional scoped-run split, and remove redundant tree/registry/scene
   effects without introducing a new scheduler or rendering architecture.
2. **Track B, experimental worktree: persistent semantic-layer backings.** Keep fine
   invalidation internally while collapsing eligible outer cacheable subtrees to one
   primary GPU composition with conservative damage repair.

Track B is the selected high-upside architecture if Track A cannot meet the 30% gate.
The tracks may compile and run non-GPU tests in parallel, but **all performance benchmarks
are serialized**.

A tempting deletion has already been rejected: removing the generic GPU exemption for
Nearby/Animation/SliderValue caused the exact RX 7900 Focus benchmark to regress from
330.29 us to 665.86 us (+101.6%). The code was restored. Do not broadly render cached
Camera geometry direct.

## Worktree and benchmark discipline

- Complete shared Phases 0 and 1 first with one writer in `/workspace/emerge-headless`.
  Validate them, then freeze the common state before either track diverges.
- Track A stays in `/workspace/emerge-headless` and has one assigned Track-A writer.
  Track B has a different assigned writer. Neither writer may edit the other worktree.
- Pause both writers before snapshotting. Record the base OID, write
  `git diff --binary HEAD` to a dated patch, and checksum it. Enumerate every copied
  untracked file in a sorted manifest with its checksum; exclude `.pi-subagents/`,
  `target/`, and generated build output.
- Create Track B at `/workspace/emerge-headless-semantic-backing` from the recorded OID,
  apply the frozen patch, copy the manifest-listed files, and verify that tracked-diff
  and fixture-manifest hashes match. Do not commit, stash, or discard the current work.
- Give every measured uncommitted variant a content snapshot ID derived from base OID,
  tracked patch hash, and untracked manifest hash. A Git SHA alone is insufficient.
- Transfer shared fixes as immutable patch files with checksums. The receiving worktree's
  assigned writer applies them while that worktree is paused.
- Add one wrapper around `/tmp/emerge-performance.lock`: builds and tests take a shared
  lock; Criterion, GPU probes, and the complete remote RPi deploy/measure window take the
  same lock exclusively. No writer may invoke build/test/benchmark commands outside it.
  Use the equivalent exclusive lock on the RPi during target measurement.
- Run benchmark order A/B/B/A and reverse it on repeat. Deploy one firmware at a time
  with fixed clocks/cooling and identical gestures.

## Phase 0: measurement contract shared by both tracks

Files: `backend/drm.rs`, `stats.rs`, `lib.rs`, Camera target diagnostics.

- [x] Rename `drm GPU queue completion span` to `GPU render elapsed`; it is
      `GL_TIME_ELAPSED_EXT`, not queue latency.
- [x] Replace the one-pending-query sampler with a bounded nonblocking query pool sampling
      every fourth rendered frame.
- [x] Add disjoint/pool-saturation counts and correlate every logged result with render
      version, cache strategy, primary image draws/pixels, draw/flush, and KMS sequence.
- [x] Log source revision (build-provided for dirty snapshots), kernel/Mesa,
      `GL_RENDERER`, `GL_VERSION`, GBM modifier, cache configuration, and video retirement
      mode. Count retirement `glFinish` fallback.
- [x] Add counters for tree scenes constructed, render-queue overwrites, scenes selected
      for draw, and scenes presented.
- [ ] From one binary capture UI-only, video-only, combined idle, combined Focus-active,
      and recovered windows. Keep Camera capture running in video-hidden controls.

Instrumentation overhead gate: <=0.25 ms GPU median and <=0.2 FPS difference versus the
old sampling cadence. If it fails, reduce cadence before using the data.

Baseline protocol: three alternating 60-second Focus-active windows after a five-second
discard, no disjoint queries, thermal throttle, GBM starvation, EBUSY, or fence/lease
imbalance.

## Phase 1: shared correctness blocker

Files: `runtime/tree_update.rs`, `events/registry_builder.rs`, `events/runtime.rs`, tests.

- [x] In `RefreshDecision::UseCachedRebuild`, refresh runtime fields before publishing
      the cached registry and replace `TreeUpdateEngine.cached_rebuild` when they change.
- [x] Keep a no-op corrective slider value registry-only; do not construct a scene.
- [x] Add the integrated tree-update sequence: native 40 -> 60, delayed controlled echo
      40, corrective 60, cached rebuild reinstall.
- [x] Assert final tree/event value 60, `patch_value: None`, no second correction,
      a fresh lane, and no no-op scene; native thumb agreement remains a hardware gate.
- [x] Prove a genuinely non-pending remote override to 25 still ends at 25.

This fix receives no performance credit. Re-freeze the baseline after it.

## Track A: deletion and redundant-work removal

### A1. Delete the unsupported scoped-run candidate

Files: `render_scene.rs`, renderer/cache tests, Focus benchmark expectations, plan notes.

- [x] Restore broad own-run coalescing and delete split-specific accounting/tests without
      spending another RPi firmware cycle on the neutral candidate.
- [x] Retain exact run keys and semantic layers. Track B represents replay scopes in its
      own backing manifest rather than forcing the main compositor to retain an extra
      payload.

RX 7900 alternating-order results were neutral and the split cannot credibly close the
4.66 ms deficit, so it receives no further performance experiment.

### A2. Delete unnecessary registry/scene effects

Files: `tree/patch.rs`, `tree/element.rs`, `tree/invalidation.rs`, `tree/layout.rs`,
`runtime/tree_update.rs`, `events/runtime.rs`.

- [x] Separate reconciliation metadata from pixel damage. Clearing or changing only
      `slider_patch_value`/origin updates cached runtime state without paint invalidation.
- [x] Do not add a lightweight acknowledgement effect: it would grow the state machine;
      reuse the refreshed cached rebuild for the existing registry-only effect.
- [x] Do not build a `RenderScene` for cache-only or registry-only effects.
- [x] Keep full registry publication whenever slider descendants' listener geometry,
      range, step, handlers, topology, focus, resize, or scale changes.
- [x] Delete duplicate cached-payload clone/install work from the reuse path.
- [x] Retain only the net-deleting Phase 1/effect-path simplification rather than adding
      effect variants or new state transitions.

Do not add within-batch slider coalescing: the existing drained batch already produces one
final effect, so it cannot reduce scene finalizations. A cross-turn render-permit scheduler
is a separate complexity experiment and is deferred unless Phase 0 proves scene selection
exceeds 1.1 constructed scenes per draw and the backing track still misses its target.

Track-A keep gates:

- cache-only corrections construct zero scenes and publish zero full registry payloads;
- >=25% lower affected tree/registry CPU p95 and >=0.50 ms end-to-end CPU p95;
- paired GPU p95 regression <=0.25 ms and idle/recovered regression <=3%;
- exact event count and final value invariants.

Track A is successful for the overall task only if the cumulative RPi build reaches
<=10.86 ms. Otherwise retain only correctness/net-deleting changes and proceed with
Track B; do not claim CPU counter reductions as GPU headroom.

## Track B: semantic-layer backing experiment

Primary files: `renderer.rs`, `render_scene.rs`, `paint_layer_payload_cache.rs`,
`tree/render.rs`, `stats.rs`, `lib.rs`, `lib/emerge_skia/options.ex`,
`lib/emerge_skia/native.ex`, `lib/emerge_skia/macos/protocol.ex`, and configuration tests.

New file: `paint_layer_backing_cache.rs`; declare it beside the existing payload cache.

### B1. Add a feature-off backing reference path

- [ ] Add `per_run | semantic_backing` strategy selection. Keep `per_run` as default and
      instant rollback until RPi acceptance.
- [ ] Back only the outermost eligible cacheable subtree: GPU-backed, finite/bounded,
      and using the existing strict unit-axis orthogonal transform predicate (absolute
      determinant 1 and integer device translation).
- [ ] Recursively reject the whole outer candidate if any descendant is Video,
      DirectOnly/DirectMedia, unresolved, or unsafe. Do not interleave a monolithic backing
      with inner per-run fallback; use the current complete ordered traversal instead.
- [ ] Flatten eligible child semantic layers into that backing while preserving all nine
      scene layers. Camera should produce one top Nearby backing and one bottom Nearby
      backing; root and Video remain direct.
- [ ] Store a backing descriptor containing local origin/bounds, pixel dimensions, scale,
      format/color space, GPU-context generation, resource generations, and every
      raster-affecting option. Descriptor change reallocates and fully redraws.
- [ ] Initially full-render each changed backing. This is a correctness/reference mode,
      not the expected final performer.
- [ ] Budget backings separately: every Surface ring slot and retained snapshot is charged;
      define entry/byte/per-entry limits and stale eviction. Release everything on strategy
      disable, renderer drop, allocation failure, or context replacement.

### B2. Conservative damage repair

- [ ] Keep the previous ordered content manifest per `PaintLayerId`. Match content identity
      using exact run/resource keys and ordered LCS; ambiguous matching falls back to full
      redraw. Stable source anchors may be added later only if diagnostics prove LCS
      fallback is material.
- [ ] Compare placement and cumulative inherited Transform/Clip/RelaxedClip/Alpha state
      separately for every content-equal match. Any relocation/state change damages both
      old and new effective bounds; payload identity intentionally excludes placement.
- [ ] Damage is the union of old and new visual bounds for changed/inserted/deleted/moved
      units, expanded for AA, text overhang, shadows/filters, and relaxed-image bleed.
- [ ] Clear damage with `BlendMode::Clear`, clip to damage, and replay the complete new
      ordered subtree through the existing direct traversal. Replaying all content under
      the damage clip preserves underlying and later-overlapping paint.
- [ ] Treat Clip, RelaxedClip, Transform, Alpha, and ShadowPass as atomic replay scopes.
      Unsafe scopes or unsupported transforms use full backing redraw or per-run fallback.
- [ ] Update the bottom backing for changing label/fill/thumb/glow without allocating a
      new immutable payload version.
- [ ] Use one explicit ownership protocol: the backing cache owns GPU `Surface` ring slots
      bound to one `DirectContext`. For each slot enforce repair -> transient snapshot ->
      primary draw -> flush/submit -> eligible reuse. Reject context mismatch/loss. Never
      mutate a slot with an application-retained snapshot. Count every Surface/snapshot
      allocation or Skia copy-on-write and fail the experiment if steady repair allocates.

### B3. Backing instrumentation and structural targets

- [ ] Count candidates/admissions/rejections, full and partial repairs, damage pixels,
      replayed nodes/scopes, primary backing draws/pixels, immutable draws/pixels,
      reallocations, fallback reason, ring slot, and resident bytes.
- [ ] Exact Camera target after warmup: <=2 backing draws and <=4 total primary UI payload
      draws, versus 29-33 today.
- [ ] Primary UI payload sampling <=0.80 million pixels; damage p95 <=25% of backing area.
- [ ] No recurring immutable stores for changing SliderValue versions and no steady
      backing allocation.

### B4. Pixel and lifecycle gates

- [ ] GPU-readback identity after warmup and sequential repairs for every Focus/Shutter
      phase; raster-only equality is insufficient because backings are GPU-only.
- [ ] Cover placement-only thumb/transform movement, deletion, resize/scale/resource and
      context-generation changes, overlapping paint, rounded/relaxed clips, exact
      0/90/180/270/reflection transforms, fractional/non-unit rejection, Alpha, escaped
      ShadowPass, subpixel text, scrolling, and Video before/between/after own runs.
- [ ] Allocation failure and unsupported content fall back for the complete outer
      candidate to current per-run rendering; video sync/retirement counters stay exact.
- [ ] Exercise on-demand screenshot capture during repair and delayed/in-flight ring reuse.
- [ ] Config default/round-trip/eviction tests pass. Ten-minute churn has bounded logical
      and GPU-resource memory with no snapshot/COW allocation after warmup.

Track-B RPi keep gate: paired active median improves at least 2.0 ms with a 95% confidence
interval excluding zero, p95 does not regress, and idle/recovered regress <=3%. Final enablement still requires the
cumulative <=10.86 ms target; structural draw-count success alone is insufficient.

## Optional small changes after the selected track

Measure independently and keep only when they close a measured remaining deficit:

1. **Sparse border payload regions:** keep the cached border raster but draw only
   conservative top/bottom/left/right source strips instead of sampling its transparent
   507k-pixel center. Prefer this to broad direct geometry. Keep at >=0.75 ms.
2. **Opaque root/video prefix:** fold a proven full-device opaque root rect into clear,
   including the exact 270-degree orthogonal transform. Mark NV12 opaque and use `Src`
   only when format, alpha, fit, clips, and device coverage prove equivalence. Keep each
   subchange at >=0.50 ms.

Do not stack neutral micro-optimizations.

## Deferred architecture: KMS media plane

Do not select direct KMS promotion for the current scene. The video is landscape NV12 and
the exact root transform is 270 degrees, while current vc4 exposes 0/180 plus reflections,
not 90/270 plane rotation.

Only reopen this path after a no-production-code `TEST_ONLY` probe and proof of one of:

- producer-native portrait/rotated NV12 with identical crop and sampling;
- a connector/mode path requiring no 90-degree plane rotation;
- another exact hardware rotation path whose measured total still meets 10.86 ms.

Any future promotion must keep atomic fallback, color range/encoding, format/modifier
intersection, acquire fences, page-flip-bound leases, alpha UI composition, cursor, and
complete screenshot fallback.

## Benchmark schedule: never concurrent

After both implementations and non-GPU validation are idle:

1. Pause both writers and acquire the exclusive `/tmp/emerge-performance.lock` through
   the wrapper; the wrapper blocks shared build/test locks.
2. Verify no unwrapped benchmark, build, test, or GPU process is active.
3. Run exact Focus and Shutter Criterion in order baseline -> Track A -> Track B ->
   Track B -> Track A -> baseline.
4. Release the exclusive lock before resuming writers or shared-lock builds/tests.
5. Reject host regressions >3%; do not accept host gains as RPi evidence.
6. Hold the coordinator lock across each complete remote deploy/measure window and use the
   target's exclusive lock. Build/deploy one firmware at a time; run A/B/B/A, then reverse.

Every RPi report includes source snapshot ID, GL renderer/version, retirement mode,
temperature/throttle state, cache/backing strategy, query cadence, draw/pixel counts,
GPU median/p95/p99, FPS, KMS sequence gaps, and producer/lease/fence counters.

## Final acceptance

- [ ] Three 60-second active Focus windows: mean/median <=10.86 ms, p95 <=11.67 ms,
      p99 <16.67 ms.
- [ ] 59.8-60.2 presented FPS and zero missed-vblank KMS sequence deltas in every
      accepted post-warmup window.
- [ ] Idle/recovered GPU elapsed regresses <=3%.
- [ ] Camera remains 59-60 FPS with no GBM, EBUSY, fence, lease, FD, or memory growth.
- [ ] Nine layers, direct Video, exact visuals/screenshots, and all slider state equality
      checks pass.
- [ ] Full default/no-default Rust tests and Clippy, Mix tests, formatting, CI, exact
      Criterion guardrails, and 30-minute target soak pass.
- [ ] Remove losing experimental branches and flags. Keep one documented runtime fallback
      only when it remains operationally necessary.

## Explicitly rejected

- Global cache disable or broad direct rendering.
- Removing the Nearby/Animation/SliderValue GPU exemption without a narrower proven policy.
- More clip rewrites, Picture/DDL, text-blob/glyph-cache duplication, hash-map tuning, or
  extra flush/finish calls as the 30% strategy.
- Lower replacement probation or immutable payload creation for every slider value.
- More Camera publication throttling, hidden visual changes, lower resolution, or altered
  sampling without explicit product approval.
- Claiming success from FPS alone, five GPU samples, or desktop Criterion.
