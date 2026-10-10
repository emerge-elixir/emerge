# Headless frame review improvements

- [x] Enforce tightly packed, nonempty RGBA/BGRA source layouts with checked sizes; reject invalid output lengths before writing. Add Rust regression tests.
- [x] Replace uniform BGRA integration coverage with distinct opaque, translucent, and transparent pixels across multiple rows, checking RGBA screenshots too.
- [x] Add an 800×1280 native copy-pipeline benchmark comparing legacy RGBA with current RGBA/BGRA. Measure throughput and Rust allocations; explicitly document the Vec stand-in for the BEAM output binary and measurement limits.
- [x] Run Rust/Elixir tests and quality checks, and record benchmark results in `bench/headless_frame_copy.md`.
- [x] After publication, bumped the Elixir video_interop minimum and lock to 0.1.3.

## Validation

- `cargo test --manifest-path native/emerge_skia/Cargo.toml`: 1,502 passed.
- Final `mix test --warnings-as-errors` with video_interop 0.1.3 and CPU DRM additions: 591 passed; BGRA validation now passes.
- `./ci-tests.sh quality`: passed.
- `cargo clippy --manifest-path native/emerge_skia/Cargo.toml --bench headless_frame_copy -- -D warnings`: passed.
- Benchmark: legacy RGBA 2.813 ms/frame, current RGBA 1.834 ms/frame, current BGRA 1.893 ms/frame; Rust frame allocations drop from three to two. Scope/limitations recorded with the results.
