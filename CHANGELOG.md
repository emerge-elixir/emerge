# Changelog

## [0.2.1] - 2026-04-17

### Changed

- Hardened native video target and NIF boundary handling, including the `submit_prime` path.
- Reduced CI noise and flakiness by gating heavier hover timing tests, relaxing one tail-clear tolerance, and downgrading routine macOS tree update logs to Elixir debug level.
- Updated macOS release and documentation flow so published HexDocs excludes internal guides and release asset verification reports visible release assets more clearly.

## [0.2.0] - 2026-04-17

### Added

- Added initial macOS support through the external macOS host runtime, using Metal when available and falling back to raster rendering when needed. `video_target` is not supported on macOS in this release.

### Changed

- Corrected wrapped row layout behavior after wrapping. Wrapped rows now respect `center_x` and `align_right` attributes from their children, and existing UIs may see visible layout changes.
