# macOS-specific follow-up

## Remaining specific checks

- Exercise two host sessions with different asset roots and registered fonts.
  Stop one, reload assets in the other, and verify renderer-local workers,
  caches, generations, and diagnostics stay independent. Include offscreen font
  isolation; this is a multi-session check, not a repeat of playback acceptance.
- Confirm gradient text, borders, shadows, and SVG behavior on both rendering
  routes. Constrained-device performance remains in
  [the gradient plan](active-universal-gradient-colors.md).
- Validate actual packaged Intel/Apple Silicon raster NIFs: architecture,
  system-library dependencies, deployment floor, and isolated BEAM
  load/render/cache/integrity probes. Host download/checksum/handshake checks
  belong to the release workflow; live playback alone is not archive validation.

## References

- [Native builds and artifact gates](../guides/reference/native-renderer-builds.md)
- [Asset ownership](../guides/internals/assets-images.md)
- [Runtime architecture](../guides/internals/architecture.md)
- [macOS worker/pacing follow-up](active-frame-production-pipeline.md)

macOS host sessions do not implement the native Linux owner/status/bounded-stop
contract. Do not turn qualification into an undocumented requirement to add it.
