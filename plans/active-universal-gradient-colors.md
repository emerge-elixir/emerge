# Gradient qualification

## Remaining gates

- [ ] Run gradient text, borders, shadows and SVG on both macOS rendering routes;
  see [macOS runtime qualification](active-macos-runtime-qualification.md).
  Linux protocol checks do not replace running the actual host.
- [ ] Qualify constrained-device performance/memory against pinned solid-color
  baselines. Include cold/warm cache behavior, SVG temporary-layer area and
  three/many-stop gradients. Follow `bench/README.md` locking and record immutable
  source identity; workstation execution smokes are not no-regression timing claims.

The independent large Borders screenshot-cache assertion/pixel work is tracked
only in [the screenshot follow-up](screenshot-cache-benchmark-investigation.md),
not as a gradient implementation failure.

## Contracts and constraints

- [Gradient design/verification](../guides/internals/gradient-colors.md)
- [Public migration](../guides/migrations/0.4.md)

Keep all UI color consumers supported. Do not substitute a gradient endpoint,
restore background-only restrictions, retired wire variants or compatibility APIs
to resolve a platform/performance issue. Preserve unrelated changes and do not
modify external applications without authorization.
