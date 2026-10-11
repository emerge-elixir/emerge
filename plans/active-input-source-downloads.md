# Build-time DRM input sources

- Replace checked-in upstream C sources, headers and quirks with the same pinned
  source archives, verified with SHA-256 before extraction.
- Cache verified archives outside the source package; support explicit offline
  builds and pre-populated caches without changing target compilation/linking.
- Retain integration patches and license notices. Generate libevdev's event-name
  header in Rust and embed quirks from the downloaded libinput archive.
- Update package closure tests and build documentation.
- Rewrite the original build commit and replay the input/runtime/docs commits;
  do not add an upstream-source addition/deletion pair to the PR history.
- Validate fresh downloads, cached/offline builds, corrupt/missing cache failures,
  bundled input tests, DRM-raster tests, Clippy and `mix test`.

## Validation

- Fresh upstream downloads and cached/offline builds pass with the original pins.
- Missing offline archives and corrupt archives fail before extraction.
- Bundled input: three runtime tests and six build-source/generator tests pass;
  generated event tables match the original upstream output byte-for-byte.
- DRM-raster: 1,573 unit tests pass, one ignored; 1 + 13 fixtures pass.
- Strict all-target Clippy passes for the input crate and DRM-raster renderer.
- The standalone source-package ExUnit test passes; full `mix test` remains
  blocked by missing dependencies.
- The daemonless probe runs; ELF dependencies remain libc/libm/libgcc/loader only.
