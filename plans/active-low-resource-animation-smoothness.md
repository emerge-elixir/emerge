# Active Plan: Low-Resource Animation Smoothness

Status: correctness and first optimization passes implemented; constrained-device cadence remains open

## Goal

Make retained transform-only Nearby animations smooth on weak DRM devices without
changing app animation specs or reintroducing first-frame flash, stale final
frames, exit-pruning bugs, or incorrect hit testing.

Target device budgets:

- transform-only pulse refresh: ideally <3 ms, acceptable <5 ms;
- patch-frame tree work during animation: ideally <8 ms, acceptable <12 ms;
- render/present within one vblank where possible.

## Implemented baseline

- Enter animations start at the first keyframe and settle to base attrs before
  pulses stop (`fe4ecac` is unrelated centered-text work and also stays covered).
- Transform-only animation dirtying preserves content paint generations and
  cached payloads while updating wrappers/registry geometry.
- Active move-x/move-y Nearby roots reuse retained moving payloads.
- Active transient pulses prepare only active animation nodes.
- Patch recompute prepares known dirty/inserted roots rather than the full tree.
- Combined render/registry traversal replaces duplicate production walks.
- Patch/animation timing and Macaw/sidepane Criterion fixtures exist.
- Rejected patch-decode cache, detached-layout restore, and unsafe refresh-local
  Nearby insertion experiments were removed.

Latest constrained-device evidence after partial attr preparation:

- patch actor about 17.4 ms;
- patch refresh about 6.6 ms;
- combined traversal about 6.5 ms;
- transform-only pulse is cheap locally, but target animation remains choppy.

## Correctness guardrails

Keep these invariants before accepting any optimization:

- inserted enter animation never flashes the final position;
- completed enter renders base attrs before `animations_active` becomes false;
- cached payload reuse cannot retain the previous sampled transform;
- exit ghost final frame/pruning is visually correct;
- interrupted enter-to-exit move-x starts from the current visual state;
- animated overlay listeners/blockers follow transformed geometry.

Add the still-missing sidepane move-x exit/prune and interrupted handoff tests
before changing exit invalidation.

## Remaining work

### 1. Re-baseline the implemented combined traversal

On the constrained target, normalize by the same number of sidepane toggles and
record:

- patch decode/apply/animation/prepare/layout/traversal/registry-post;
- pulse prepare/traversal/render/present;
- scenes constructed, queue overwrites, scenes drawn/presented;
- cache hits/stores/stale evictions and animation frame count.

Do not optimize stale pre-combined numbers.

### 2. Reduce transform-only registry work

Audit whether active animation frames can reuse listener/action definitions and
update only geometry for:

- the animated subtree;
- affected Nearby blockers;
- relevant hover/focus entries.

Keep full registry rebuild for topology, listener, focus, range, or unrelated
geometry changes. Differential tests must prove exact event precedence and hit
results throughout start/mid/final samples.

### 3. Revisit first-mount cost only with safe geometry

The current animated Nearby insert uses `Resolve` because the refresh-local
shortcut caused a device freeze and had no significant local win. Reopen only if
new profiling shows first-mount resolve is still material.

Any replacement must:

- classify transform-only versus paint/layout animation;
- establish geometry and animation runtime before the first render;
- fall back to Resolve when geometry is absent or layout-affecting;
- pass repeated second-open and hit-test stress on target.

### 4. Coalesce superseded patch renders

If diagnostics show more than one scene constructed per selected draw while an
animation is active:

- drain/coalesce compatible queued patches before rendering;
- skip only superseded intermediate visual states;
- preserve final semantic state, input/registry correctness, and animation
  completion;
- do not add a scheduler if scene construction already tracks selected draws.

### 5. Pacing check after CPU work

Keep DRM page-flip backpressure and physical mode interval prediction. Revisit
backend timing only after tree/refresh work is under budget and cadence still
jumps. Validate that `display` remains physical refresh while achieved frame
rate is reported separately.

## Acceptance

- First, middle, final, exit, interrupted, and hit-test guardrails pass.
- Target pulse refresh is <=5 ms and patch tree work <=12 ms, or measurements
  identify a smaller proven bottleneck with an approved follow-up.
- Repeated 125 ms sidepane animations produce cadence close to physical refresh
  without stale cache churn.
- Desktop behavior and unrelated patch/event paths do not regress.
- Rejected experiments and temporary diagnostics are removed or clearly gated.

## Validation

```bash
cargo fmt --manifest-path native/emerge_skia/Cargo.toml -- --check
cargo test --manifest-path native/emerge_skia/Cargo.toml
cargo test --manifest-path native/emerge_skia/Cargo.toml --no-default-features --features drm
cargo clippy --manifest-path native/emerge_skia/Cargo.toml -- -D warnings
mix format --check-formatted
mix test
git diff --check
```

Run Criterion and target measurements only through the performance lock.
