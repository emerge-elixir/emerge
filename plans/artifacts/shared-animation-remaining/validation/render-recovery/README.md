# D15 binary headless render recovery

Base commit: `33ee559`; D12–D15 remain uncommitted.
Functional identity: `source-sha256:23d37e6a138c376a16e0f7e76407b898d9ecf39a1c0508d67039802afce9572d`.

## Reproduction and change

`before.log` runs the extracted production binary loop with its unchanged error
behavior: a controlled draw/readback failure discards a terminal frame and never
retries without another tree message. Extraction supplies a native closure test seam,
not a new NIF, backend implementation or production fault switch.

The loop now retains only the newest failed render state. Autonomous retry delays
are 16/32/64/128/250 ms, capped at 250 ms. A new scene attempts immediately and
synchronously replaces the pending state. Success resets backoff and resumes normal
native wall-clock animation pulses; failure advances no output sequence. Static
recovery invents no pulse. Stop and disconnection remain selectable between attempts.

## Directed qualification

Six new unit functions:
- Failed terminal frame recovers without another tree message.
- Virtual 1000 ms schedule produces eight autonomous attempts; cap/reset checks.
- 64 failed-state replacements release superseded payloads synchronously; successful
  output does not retain retry history. Weak payload probes are not total heap/GPU data.
- Controlled pending Stop/disconnection release the retained state without retry.
- Recovered animation preserves pipeline timing and resumes native pulses.
- Twelve real tree-actor/direct/headless traces: pre/post CPU-raster draw fault ×
  1/8/32 updates × same-mount/remount. Includes text/drag/release/Nearby, stale-command
  rejection, registry/IME parity, final sample removal, latest recovered pixels versus
  fresh/retained rendering, and unchanged old-scene replay. Installed input state
  progresses while output remains failed; this is not a display acknowledgment.

## Validation

- `cargo.log`: **1426 Rust units + 14 integration**.
- `clippy.log`: default features, benches/tests/`bench-diagnostics`, warnings denied.
- `mix.log`: source-built release NIF, **520** tests/doctests, 9 excluded.
- `ci.log`: `./ci-tests.sh all`, **1426 + 14** Rust, **526** Elixir tests/doctests,
  3 excluded, **0** Dialyzer errors; formatting/quality pass.
- `headless-all-check.log`: locked compile check with tests and `headless-all`.
  Passes with an existing unused Vulkan capability-method warning; this is not an
  all-feature denied-warning lint or device-execution claim.
- `source.sha256`: D14 selected inputs plus binary orchestration and joint recovery
  test modules. Its digest is the identity above; `changes-from-D14.txt` names changes.
- `binaries.sha256`: tested release NIF and default-feature debug unit executable.

Run manifests from the repository root. Historical console EOFs are preserved.

## Limits

No package, performance, memory or platform gate closes. Faults are injected around
native CPU-raster drawing, not actual GPU/context failures. Persistent backend errors
can remain pending until success or Stop; retry does not reconstruct a lost context.
PRIME terminal synchronization, packed conversion/delivery failure, blocked driver
calls, broader ghost/virtual-key/inertia histories and physical presentation remain
unqualified. Existing binary conversion/latest-frame/Rustler delivery behavior is
unchanged outside draw/readback error handling. Failed-attempt metrics, prolonged
buffering, live/peak retention and synchronous disposal still need full accounting
and new locked benchmarks. One retry slot does not bound queues or establish a
whole-runtime memory budget. No new NIF/atom/wire version, production thread, lock,
global cache or native animation clock. No commit or push.
