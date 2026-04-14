# CLAUDE.md

This file provides guidance to AI agents when working with code in this repository.

## Build & Development Commands

```bash
mix deps.get                    # Install Elixir dependencies
mix compile                     # Compile Elixir + Rust NIF (first build downloads Skia binaries)
mix test                        # Run tests
mix docs                        # Generate ExDoc documentation
cd ../emerge_demo && iex -S mix # Run the standalone demo app from https://github.com/emerge-elixir/emerge_demo
./ci-tests.sh                   # Run the full CI check suite locally

# Rust-specific (from native/emerge_skia/)
cargo clippy                    # Lint Rust code
cargo clippy -- -D warnings     # Lint with warnings as errors
cargo build --release           # Build release (mix compile does this automatically)
```

## Testing Expectations

- Always run `cargo test` and `mix test` after implementing changes.
- Prefer `./ci-tests.sh` when validating full local CI coverage (`quality`, `test`, `dialyzer`, or `all`).

## Architecture

Emerge is a GUI framework for elixir that sits on top of EmergeSkia
EmergeSkia is a layout engine and input system in Rust using Skia to renderer on various backend.
EmergeSkia is implemented as Rustler NIF to bridge Elixir and Rust.

### Data Flow

Rendering
```
Elixir: EmergeSkia.upload_tree(renderer, tree)
    │
    │  EMRG binary decoded to ElementTree
    ▼
Rust tree actor: layout_and_refresh_default(tree, constraint, scale)
    │
    │  produces RenderScene + event rebuild
    ▼
Render thread: SkiaRenderer.render() draws to GPU surface
    │
    ▼
backend: smithay-client-toolkit Wayland / libdrm / raster...

```

Events
```
backend: Wayland seat events / libinput
    │
    │ Raw Input Events 
    ▼
Rust event actor: event processing and translation
    │
    │  TreeActorEvents/Elixir messages
    ▼
TreeActor:rerender / Elixir: event forwarding 

```

### Key Components

**Elixir Side:**
- `EmergeSkia` (`lib/emerge_skia.ex`) - Public API: `start/1`, `upload_tree/2`, `patch_tree/3`, `render_to_pixels/2`, `measure_text/2`, `stop/1`
- `EmergeSkia.Native` (`lib/emerge_skia/native.ex`) - Rustler NIF bindings

**Rust Side** (`native/emerge_skia/src/lib.rs`):
- `SkiaRenderer` - wraps Skia surface/context, executes render scenes
- `RendererResource` - NIF resource holding render state and event proxy
- `WaylandApp` - smithay-client-toolkit app state managing Wayland events
- `LayoutOutput` struct - bundles render commands + event registry, returned by `refresh()` and `layout_and_refresh_default()`
- `refresh()` / `layout_and_refresh_default()` - produce both outputs after DOM/scroll changes

### Threading Model

The NIF spawns dedicated native threads for the backend event loop and runtime actors. Communication happens via:
- `Arc<Mutex<RendererState>>` - commands from Elixir to render thread
- `EventLoopProxy<UserEvent>` - signals (Redraw/Stop) from Elixir to event loop

### Color Format

Colors are `u32` in RGBA format: `0xRRGGBBAA`. Use `EmergeSkia.rgb/3` or `EmergeSkia.rgba/4` helpers.

### Draw Commands

Commands are Elixir tuples decoded in Rust:
- `{:rect, x, y, w, h, fill}` / `{:rounded_rect, x, y, w, h, radius, fill}`
- `{:border, x, y, w, h, radius, stroke_width, color}`
- `{:text, x, y, string, font_size, fill}`
- `{:gradient, x, y, w, h, from_color, to_color, angle_degrees}`
- `{:push_clip, x, y, w, h}` / `:pop_clip`
- `{:translate, x, y}` / `:save` / `:restore`

### Platform Support

Linux only (Wayland and DRM). The window backend uses smithay-client-toolkit plus EGL/Skia on Wayland.

## Related Projects

- **elm-ui** (`/workspace/elm-ui`) - Elm ui repository. Study this for elm-ui reference.

## Target Architecture

The goal is to match scenic_driver_skia's architecture: **one renderer, multiple backends**.

### Backends Needed

1. **Wayland** - Windowed EGL/Skia surface via smithay-client-toolkit
2. **DRM** - Direct framebuffer rendering for embedded/kiosk (no window manager)
3. **Raster** - Offscreen CPU rendering to RGB buffer (for testing/headless)

### Reference: scenic_driver_skia Structure

```
native/scenic_driver_skia/src/
├── lib.rs           # NIF entry, script parsing
├── renderer.rs      # Core SkiaRenderer (backend-agnostic drawing)
├── backend.rs       # Wayland backend
├── drm_backend.rs   # DRM backend (direct framebuffer)
├── raster_backend.rs # Offscreen RGB buffer
└── input.rs         # Input event handling
```

Key pattern: `Renderer` struct is backend-agnostic, backends provide the Skia `Surface` and handle input/display.

## Documentation

All docs currently live in `guides/internals/` (architecture, EMRG format, events, scrolling, tree patching).
Run `mix docs` to generate the full ExDoc site.

## Repository Coding Preferences

- Default to functional composition for collection building (`map`, `filter`, `flat_map`, `fold`, `collect`) instead of mutable accumulator loops.
- Avoid mutable accumulator patterns in general (for example `let mut out = Vec::new(); for ... { out.push(...) }`).
- Prefer functions that return collections over functions that mutate passed-in output collections.
- Avoid Elixir aliases with `as:`. Prefer the original module name or a normal alias without renaming.

## Git Commit Guidelines

- Do NOT include `Co-Authored-By` lines in commit messages
