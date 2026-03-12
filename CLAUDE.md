# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
mix deps.get                    # Install Elixir dependencies
mix compile                     # Compile Elixir + Rust NIF (first build downloads Skia binaries)
mix test                        # Run tests
mix docs                        # Generate ExDoc documentation
mix run demo.exs                # Run the demo (requires display: X11 or Wayland)

# Rust-specific (from native/emerge_skia/)
cargo clippy                    # Lint Rust code
cargo clippy -- -D warnings     # Lint with warnings as errors
cargo build --release           # Build release (mix compile does this automatically)
```

## Testing Expectations

- Always run `cargo test` and `mix test` after implementing changes.

## Architecture

EmergeSkia is a minimal Skia renderer for Elixir, designed as a lightweight alternative to Scenic for the Emerge layout engine. It uses Rustler to bridge Elixir and Rust.

### Data Flow

```
Elixir: EmergeSkia.render(renderer, commands)
    │
    │  Vec<DrawCmd> decoded via manual Decoder impl
    ▼
Rust NIF: commands stored in Arc<Mutex<RendererState>>
    │
    │  Event proxy triggers redraw
    ▼
Render thread: SkiaRenderer.render() draws to GPU surface
    │
    ▼
winit window (Wayland)
```

### Key Components

**Elixir Side:**
- `EmergeSkia` (`lib/emerge_skia.ex`) - Public API: `start/3`, `render/2`, `measure_text/2`, `stop/1`
- `EmergeSkia.Native` (`lib/emerge_skia/native.ex`) - Rustler NIF bindings

**Rust Side** (`native/emerge_skia/src/lib.rs`):
- `DrawCmd` enum with manual `Decoder` impl - handles tuples >7 elements that `NifTaggedEnum` can't
- `SkiaRenderer` - wraps Skia surface/context, executes draw commands
- `RendererResource` - NIF resource holding render state and event proxy
- `App` - winit `ApplicationHandler` managing window events
- `LayoutOutput` struct - bundles render commands + event registry, returned by `refresh()` and `layout_and_refresh_default()`
- `refresh()` / `layout_and_refresh_default()` - produce both outputs after DOM/scroll changes

### Threading Model

The NIF spawns a dedicated thread for the winit event loop (required because BEAM owns the main thread). Communication happens via:
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

Linux only (X11 and Wayland). The Cargo.toml enables both backends via skia-safe features. The event loop uses `EventLoopBuilderExtX11::with_any_thread(true)` to run on non-main threads.

## Related Projects

- **elm-ui** (`/workspace/elm-ui`) - Elm ui repository. Study this for elm-ui reference.

## Target Architecture

The goal is to match scenic_driver_skia's architecture: **one renderer, multiple backends**.

### Backends Needed

1. **Wayland** - Windowed GL surface (current implementation uses winit/glutin)
2. **DRM** - Direct framebuffer rendering for embedded/kiosk (no window manager)
3. **Raster** - Offscreen CPU rendering to RGB buffer (for testing/headless)

### Reference: scenic_driver_skia Structure

```
native/scenic_driver_skia/src/
├── lib.rs           # NIF entry, script parsing
├── renderer.rs      # Core SkiaRenderer (backend-agnostic drawing)
├── backend.rs       # Wayland backend (winit/glutin)
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

## Git Commit Guidelines

- Do NOT include `Co-Authored-By` lines in commit messages
