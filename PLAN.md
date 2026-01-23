# PLAN.md

Implementation plan for multi-backend architecture in emerge_skia.

## Current State

Single monolithic `lib.rs` (~750 lines) combining:
- Draw command decoding
- Skia rendering
- Wayland/X11 windowing (via winit/glutin)
- NIF interface

No input event support, no backend abstraction.

## Target Architecture

```
lib.rs (NIF entry, DrawCmd, command decoding)
    │
    ▼
renderer.rs (backend-agnostic SkiaRenderer)
    │
    ├── backend/wayland.rs (winit/glutin GL window)
    ├── backend/drm.rs (direct framebuffer, evdev input)
    └── backend/raster.rs (offscreen CPU surface)
    │
input.rs (InputEvent enum, InputQueue, encoding)
```

## Phase 1: Refactor Current Code

**Goal:** Extract renderer and backend without changing functionality.

### 1.1 Create `renderer.rs`

Extract from `lib.rs`:
- `DrawCmd` enum and `Decoder` impl
- `RendererState` struct
- `SkiaRenderer` struct (rename from current impl)
- `create_skia_surface()` function
- `color_from_u32()` helper
- `get_default_typeface()` and font cache

The renderer should be backend-agnostic:
```rust
pub struct Renderer {
    surface: Surface,
    gr_context: Option<DirectContext>,
    source: SurfaceSource,  // GL or Raster
}

impl Renderer {
    // For GPU backends (Wayland, DRM with GPU)
    pub fn new_gl(dimensions, fb_info, gr_context, samples, stencil) -> Self;

    // For CPU backends (Raster)
    pub fn from_surface(surface: Surface) -> Self;

    pub fn render(&mut self, commands: &[DrawCmd]);
    pub fn resize(&mut self, dimensions: (u32, u32));
    pub fn surface_mut(&mut self) -> &mut Surface;
}
```

### 1.2 Create `backend/wayland.rs`

Move from `lib.rs`:
- `UserEvent` enum
- `Env_` struct (rename to `GlEnv`)
- `App` struct and `ApplicationHandler` impl
- `create_window_and_renderer()` function
- Event loop setup with `with_any_thread`

Public interface:
```rust
pub fn run(
    config: WaylandConfig,
    render_state: Arc<Mutex<RendererState>>,
    running_flag: Arc<AtomicBool>,
    event_proxy_tx: Sender<EventLoopProxy<UserEvent>>,
);
```

### 1.3 Update `lib.rs`

Keep only:
- NIF functions (`start`, `stop`, `render`, `measure_text`, `is_running`)
- `RendererResource` struct
- Module declarations and NIF registration
- Atoms

## Phase 2: Add Raster Backend ✓

**Goal:** Offscreen rendering for testing/headless use.

**Implementation Note:** Used a simplified synchronous API instead of stateful resource
because Skia surfaces are not Send+Sync (can't be shared across threads safely).

### 2.1 Create `backend/raster.rs` ✓

Created `RasterBackend` struct with CPU-backed Skia surface:
```rust
pub struct RasterBackend { ... }

impl RasterBackend {
    pub fn new(config: &RasterConfig) -> Result<Self, String>;
    pub fn render(&mut self, state: &RenderState) -> RasterFrame;
}

pub struct RasterFrame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,  // RGBA bytes
}
```

### 2.2 Add NIF function ✓

Single synchronous function (simpler than stateful resource):
```rust
#[rustler::nif]
fn render_to_pixels(env: Env, width: u32, height: u32, commands: Vec<DrawCmd>) -> NifResult<Binary>;
```

### 2.3 Elixir API ✓

```elixir
@spec render_to_pixels(non_neg_integer(), non_neg_integer(), list()) :: binary()
def render_to_pixels(width, height, commands)
```

## Phase 3: Add Input Support ✓

**Goal:** Mouse/keyboard events for interactive UIs.

**Implementation Note:** Input events are captured by the Wayland backend and stored in an
`InputQueue` shared between the NIF and the backend thread. Events can be polled via
`drain_input_events/1` or pushed to a process via `set_input_target/2` notifications.

### 3.1 Create `input.rs` ✓

```rust
pub enum InputEvent {
    CursorPos { x: f32, y: f32 },
    CursorButton { button: MouseButton, action: Action, mods: Mods, x: f32, y: f32 },
    CursorScroll { dx: f32, dy: f32, x: f32, y: f32 },
    Key { key: Key, action: Action, mods: Mods },
    Codepoint { char: char, mods: Mods },
    ViewportResize { width: u32, height: u32, scale: f32 },
}

pub struct InputQueue { ... }
```

### 3.2 Update Wayland backend ✓

Added to `App`:
- `input_queue: Arc<Mutex<InputQueue>>`
- `current_mods: u8` for tracking modifier key state
- Handle `WindowEvent::CursorMoved`, `MouseInput`, `MouseWheel`, `KeyboardInput`, `ModifiersChanged`, `Focused`, `CursorEntered`, `CursorLeft`

### 3.3 Add NIF functions ✓

```rust
#[rustler::nif]
fn set_input_mask(renderer, mask: u32) -> Atom;

#[rustler::nif]
fn drain_input_events(renderer) -> Vec<InputEvent>;

#[rustler::nif]
fn set_input_target(renderer, pid: Option<LocalPid>) -> Atom;
```

### 3.4 Elixir API ✓

```elixir
# Input mask constants
def input_mask_key/0, input_mask_codepoint/0, input_mask_cursor_pos/0, etc.

# Functions
def set_input_mask(renderer, mask)
def drain_input_events(renderer)
def set_input_target(renderer, pid)
```

## Phase 4: Add DRM Backend

**Goal:** Direct framebuffer rendering for embedded/kiosk.

### 4.1 Create `backend/drm.rs`

Reference: `/workspace/scenic_driver_skia/native/scenic_driver_skia/src/drm_backend.rs`

Components:
- DRM device/connector/CRTC setup
- GBM surface for Skia
- Page flipping
- Optional hardware cursor

### 4.2 Create `drm_input.rs`

Reference: `/workspace/scenic_driver_skia/native/scenic_driver_skia/src/drm_input.rs`

- evdev device enumeration
- Keyboard/mouse event translation
- Hotplug support

### 4.3 Add dependencies to Cargo.toml

```toml
drm = "0.14"
gbm = { version = "0.18", features = ["drm-support"] }
evdev = "0.12"
```

### 4.4 NIF functions

```rust
#[rustler::nif]
fn start_drm(config: DrmConfig) -> NifResult<ResourceArc<DrmResource>>;
```

## Phase 5: Unified Backend Selection

**Goal:** Single `start/2` with backend option.

### 5.1 Backend enum

```elixir
@type backend :: :wayland | :drm | :raster
@type config :: [
  backend: backend(),
  width: integer(),
  height: integer(),
  title: String.t(),  # wayland only
  drm_device: String.t(),  # drm only, e.g. "/dev/dri/card0"
]

def start(config \\ [])
```

### 5.2 Rust side

```rust
#[derive(NifTaggedEnum)]
enum BackendConfig {
    Wayland { width: u32, height: u32, title: String },
    Drm { width: u32, height: u32, device: String },
    Raster { width: u32, height: u32 },
}

#[rustler::nif]
fn start(config: BackendConfig) -> NifResult<ResourceArc<RendererResource>>;
```

## File Structure (Final)

```
native/emerge_skia/src/
├── lib.rs              # NIF entry, resource types, registration
├── renderer.rs         # DrawCmd, SkiaRenderer, font cache
├── input.rs            # InputEvent, InputQueue, encoding
├── backend/
│   ├── mod.rs
│   ├── wayland.rs      # winit/glutin windowed
│   ├── drm.rs          # direct framebuffer
│   └── raster.rs       # offscreen CPU
└── drm_input.rs        # evdev input for DRM backend
```

## Dependencies (Final Cargo.toml)

```toml
[dependencies]
rustler = "0.37"

# Windowing (Wayland backend)
winit = "0.30"
glutin = "0.32"
glutin-winit = "0.5"
raw-window-handle = "0.6"
gl = "0.14"

# DRM backend
drm = "0.14"
gbm = { version = "0.18", features = ["drm-support"] }
evdev = "0.12"
libc = "0.2"

# Skia
skia-safe = { version = "0.91.1", default-features = false, features = [
    "x11", "wayland", "embed-freetype", "binary-cache"
] }
```

## Testing Strategy

1. **Raster backend** - Unit tests with pixel comparison
2. **Wayland backend** - Manual testing, CI with virtual framebuffer (Xvfb)
3. **DRM backend** - Manual testing on target hardware

## Milestones

- [x] Phase 1: Refactor (renderer.rs, backend/wayland.rs)
- [x] Phase 2: Raster backend
- [x] Phase 3: Input support
- [ ] Phase 4: DRM backend
- [ ] Phase 5: Unified API
