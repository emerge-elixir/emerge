# macOS Backend

This note describes the current macOS implementation in `EmergeSkia`.

## Summary

macOS is implemented through an external native host process, not an in-process
AppKit NIF backend.

The public Elixir API stays:

- `EmergeSkia.start(backend: :macos, ...)`
- `EmergeSkia.stop/1`
- `EmergeSkia.running?/1`
- `EmergeSkia.upload_tree/2`
- `EmergeSkia.patch_tree/3`

Internally, Elixir connects to one shared macOS host process over a Unix domain
socket and creates one session/window per renderer.

## Why External Host

The BEAM does not give the NIF reliable ownership of the macOS process main
thread.

Probe results showed:

- NIF load is not on the process main thread
- regular NIF calls are not on the process main thread
- dirty NIF calls are not on the process main thread
- NIF-spawned threads are not on the process main thread

That makes a direct AppKit window backend inside the NIF the wrong foundation.

The external host avoids that problem by letting a separate native process own
AppKit lifecycle correctly.

## Architecture

### Elixir side

- `lib/emerge_skia.ex`
  routes `backend: :macos` through the macOS host path
- `lib/emerge_skia/macos/host.ex`
  owns host discovery, startup, protocol I/O, and session lifecycle
- `lib/emerge_skia/macos/renderer.ex`
  is the Elixir renderer handle for macOS sessions

### Native side

- `native/emerge_skia/src/bin/macos_host.rs`
  is the real macOS backend host
- `native/emerge_skia/src/renderer.rs`
  remains backend-agnostic drawing code
- `native/emerge_skia/src/events/*`
  remains the shared input/event runtime used by macOS too

### Process model

- one shared macOS host process per workspace/runtime
- one session/window per `EmergeSkia.start/1`
- host remains alive across session restarts
- Elixir-side renderer stop closes only that session

## Backend Selection

macOS supports:

- `macos_backend: :auto`
- `macos_backend: :metal`
- `macos_backend: :raster`

` :auto` prefers Metal and falls back to raster when Metal is unavailable.

## Assets And Fonts

The external macOS host now receives merged asset config from Elixir session
startup.

That includes:

- asset source roots
- runtime asset policy
- runtime allowlist/extensions
- runtime max file size
- preloaded custom fonts

The host starts the shared asset worker and rerenders sessions when async asset
state changes arrive.

## Input Model

macOS uses a custom AppKit `NSView` that is the first responder and implements
`NSTextInputClient`.

Current text/input path includes:

- raw pointer events
- raw key events
- `insertText:` commits
- `setMarkedText:` / `unmarkText` preedit
- menu and selector-driven commands like copy/cut/paste/select-all
- word/paragraph/document navigation and deletion selectors
- replacement-range-aware text/preedit application

Focused text inputs use shared runtime reconciliation with tree-side
`patch_content`, so incoming tree patches do not visibly override local editing
until reconciliation resolves them.

## Clipboard

macOS now uses the real system clipboard through `NSPasteboard` for text input
commands.

## Unsupported For Now

- video targets on macOS
- precompiled Darwin artifacts

`EmergeSkia.video_target/2` intentionally returns an error for macOS.

## Validation

The supported runtime path is the shared external host plus normal
`EmergeSkia.start/1` session creation.

Manual smoke validation should happen through `emerge_demo`, not through
standalone probe or wrapper binaries.

## Next Work

Most remaining macOS work is cleanup and parity work, not foundational backend
architecture.

High-value remaining items:

- documentation cleanup around probes/smokes as code evolves
- host-level automated integration coverage
- distribution/productization when needed
