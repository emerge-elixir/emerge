# PLAN.md

Implementation plan for emerge_skia - a Skia renderer for Elixir with EMRG tree integration.

## Current State

Multi-backend Skia renderer with:
- Draw command decoding and rendering
- Wayland/X11 windowing (via winit/glutin)
- Raster (offscreen CPU) backend
- Push-based input event delivery
- **NEW:** EMRG tree deserialization and patching
- **NEW:** Elixir-side tree definition + EMRG encoder (no layout compute)

## Architecture

```
lib.rs (NIF entry, resources, registration)
    │
    ├── renderer.rs (DrawCmd, RenderState, font cache)
    │
    ├── backend/
    │   ├── wayland.rs (winit/glutin GL window)
    │   ├── drm.rs (planned: direct framebuffer)
    │   └── raster.rs (offscreen CPU surface)
    │
    ├── input.rs (InputEvent, InputHandler, push delivery)
    │
    └── tree/
        ├── mod.rs (public exports)
        ├── element.rs (Element, ElementTree, Frame)
        ├── attrs.rs (Attrs, Length, Color, etc.)
        ├── deserialize.rs (EMRG binary parser)
        ├── patch.rs (incremental tree updates)
        └── layout.rs (two-pass layout algorithm)
```

---

## Completed Phases

### Phase 1: Refactor ✓

Extracted renderer and backend modules from monolithic lib.rs.

### Phase 2: Raster Backend ✓

Offscreen CPU rendering via `render_to_pixels/3` NIF.

### Phase 3: Input Support ✓

Push-based input events via `{:emerge_skia_event, event}` messages.

Input event types:
- `{:cursor_pos, x, y}`
- `{:cursor_button, button, action, mods, x, y}`
- `{:cursor_scroll, dx, dy, x, y}`
- `{:key, key, scancode, action, mods}`
- `{:codepoint, char, mods}`
- `{:viewport_resize, width, height, scale}`
- `{:focused, bool}`
- `{:cursor_entered}` / `{:cursor_left}`

### Phase 4: Emerge Tree Integration ✓

#### 4.1 EMRG Binary Format (v2)

Header:
```
"EMRG"            # 4 bytes magic
version           # 1 byte (currently 2)
node_count        # 4 bytes BE
```

Node record:
```
id_len            # 4 bytes BE
id_bin            # Erlang term_to_binary
type_tag          # 1 byte (row=1, wrapped_row=2, column=3, el=4, text=5, none=6)
attrs_len         # 4 bytes BE
attrs_bin         # Typed attribute block (see below)
child_count       # 2 bytes BE
children...       # Length-prefixed child IDs
```

Attribute block:
```
attr_count        # 2 bytes BE
attr_records...   # tag (1 byte) + value (varies)
```

#### 4.2 Attribute Tags

| Tag | Attribute | Value Encoding |
|-----|-----------|----------------|
| 1 | width | Length: 0=fill, 1=content, 2=px+f64, 3=fill_portion+f64 |
| 2 | height | Length (same as width) |
| 3 | padding | 0=uniform+f64, 1/2=sides+4×f64 |
| 4 | spacing | f64 |
| 5 | align_x | 0=left, 1=center, 2=right |
| 6 | align_y | 0=top, 1=center, 2=bottom |
| 7 | scrollbar_y | bool |
| 8 | scrollbar_x | bool |
| 9 | clip | bool |
| 10 | clip_y | bool |
| 11 | clip_x | bool |
| 12 | background | 0=color, 1=gradient(color+color+f64) |
| 13 | border_radius | f64 |
| 14 | border_width | f64 |
| 15 | border_color | Color |
| 16 | font_size | f64 |
| 17 | font_color | Color |
| 18 | font | 0=atom, 1=string (u16 len + bytes) |
| 19 | font_weight | atom (u16 len + bytes) |
| 20 | font_style | atom (u16 len + bytes) |
| 21 | content | u16 len + UTF-8 bytes |
| 22-27 | nearby (above/below/on_left/on_right/in_front/behind) | u32 len + EMRG subtree |
| 28 | snap_layout | bool |
| 29 | snap_text_metrics | bool |

Color encoding: 0=rgb(3×u8), 1=rgba(4×u8), 2=named(u16 len + bytes)

Runtime-only attrs are not encoded (`scroll_x`, `scroll_y`, `scroll_max`, `scroll_bounds`, `clip_bounds`,
`clip_content`, `text_baseline_offset`, `scroll_capture`, `__layer`, `__attrs_hash`, `nearby_*`).

#### 4.3 Tree NIF Functions

```elixir
tree_new()                        # Create empty tree resource
tree_upload(tree, binary)         # Upload EMRG binary, replaces contents
tree_patch(tree, binary)          # Apply incremental patches
tree_layout(tree, width, height)  # Compute layout, returns frame tuples
tree_node_count(tree)             # Get node count
tree_is_empty(tree)               # Check if empty
tree_clear(tree)                  # Clear all nodes
```

#### 4.4 Patch Operations

| Tag | Operation | Format |
|-----|-----------|--------|
| 1 | SetAttrs | id_len + id + attr_len + attrs |
| 2 | SetChildren | id_len + id + count + child_ids |
| 3 | InsertSubtree | parent_len + parent_id + index + tree_len + tree_bytes |
| 4 | Remove | id_len + id |

### Phase 5: Layout Engine (Rust) ✓

Layout is implemented in Rust (`native/emerge_skia/src/tree/layout.rs`).
Elixir provides only the tree definition + EMRG encoder/patcher.

#### Rust Types

```rust
pub struct Constraint {
    pub max_width: f32,
    pub max_height: f32,
}

pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct Attrs {
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub padding: Option<Padding>,
    pub spacing: Option<f64>,
    pub align_x: Option<AlignX>,
    pub align_y: Option<AlignY>,
    pub background: Option<Background>,
    pub border_radius: Option<f64>,
    pub border_width: Option<f64>,
    pub border_color: Option<Color>,
    pub font_size: Option<f64>,
    pub font_color: Option<Color>,
    pub content: Option<String>,
    // ... and more
}
```

---

## Remaining Phases

### Phase 6: Tree Rendering

**Goal:** Generate DrawCmd list from laid-out ElementTree.

#### 6.1 Render traversal

```rust
fn render_tree(tree: &ElementTree) -> Vec<DrawCmd> {
    let mut commands = Vec::new();
    if let Some(root_id) = &tree.root {
        render_element(tree, root_id, &mut commands);
    }
    commands
}

fn render_element(tree: &ElementTree, id: &ElementId, commands: &mut Vec<DrawCmd>) {
    let element = tree.get(id)?;
    let frame = element.frame?;
    let attrs = &element.attrs;

    // Background
    if let Some(bg) = &attrs.background {
        match bg {
            Background::Color(c) => commands.push(DrawCmd::Rect { ... }),
            Background::Gradient { from, to, angle } => commands.push(DrawCmd::Gradient { ... }),
        }
    }

    // Border
    if let Some(width) = attrs.border_width {
        commands.push(DrawCmd::Border { ... });
    }

    // Text content
    if element.kind == ElementKind::Text {
        if let Some(content) = &attrs.content {
            commands.push(DrawCmd::Text { ... });
        }
    }

    // Clipping
    if attrs.clip.unwrap_or(false) {
        commands.push(DrawCmd::PushClip { ... });
    }

    // Render children
    for child_id in &element.children {
        render_element(tree, child_id, commands);
    }

    if attrs.clip.unwrap_or(false) {
        commands.push(DrawCmd::PopClip);
    }
}
```

#### 6.2 NIF function

```rust
#[rustler::nif]
fn tree_render(tree_res: ResourceArc<TreeResource>) -> Vec<DrawCmd>;
```

### Phase 7: Direct Tree Rendering

**Goal:** Render tree directly to Skia surface (skip DrawCmd intermediate).

More efficient for large trees - avoid allocating command vector.

```rust
fn render_tree_direct(tree: &ElementTree, canvas: &Canvas) {
    // Traverse and draw directly
}
```

### Phase 8: Scrolling Support

**Goal:** Handle scroll offsets and clip bounds.

- Track `scroll_x`, `scroll_y` offsets per element
- Compute `scroll_max` from content overflow
- Apply scroll transform during rendering
- Handle scroll input events

### Phase 9: DRM Backend

**Goal:** Direct framebuffer rendering for embedded/kiosk.

Reference: `/workspace/scenic_driver_skia/native/scenic_driver_skia/src/drm_backend.rs`

- DRM device/connector/CRTC setup
- GBM surface for Skia
- evdev input handling
- Page flipping

### Phase 10: Unified Backend Selection

**Goal:** Single `start/2` with backend option.

```elixir
def start(config \\ [])
# config: [backend: :wayland | :drm | :raster, width: int, height: int, ...]
```

---

## File Structure

```
native/emerge_skia/src/
├── lib.rs              # NIF entry, resources, registration
├── renderer.rs         # DrawCmd, RenderState, font cache
├── input.rs            # InputEvent, InputHandler
├── backend/
│   ├── mod.rs
│   ├── wayland.rs      # winit/glutin windowed
│   ├── drm.rs          # direct framebuffer (planned)
│   └── raster.rs       # offscreen CPU
└── tree/
    ├── mod.rs          # Public exports
    ├── element.rs      # Element, ElementTree, Frame, ElementKind
    ├── attrs.rs        # Attrs, Length, Padding, Color, Background, etc.
    ├── deserialize.rs  # EMRG binary format parser
    ├── patch.rs        # Patch decoding and application
    └── layout.rs       # Two-pass layout algorithm
```

## Testing

- **Rust unit tests**: `cargo test` in native/emerge_skia/
- **Elixir integration tests**: `mix test`
- **Manual testing**: `mix run demo.exs`

Current test counts:
- 20 Rust tests (attrs, deserialize, element, layout, patch)
- 11 Elixir tests (tree operations, layout)

---

## Milestones

- [x] Phase 1: Refactor (renderer.rs, backend separation)
- [x] Phase 2: Raster backend
- [x] Phase 3: Input support (push-based)
- [x] Phase 4: Emerge tree integration (EMRG deserialization)
- [x] Phase 5: Layout engine (two-pass algorithm)
- [ ] Phase 6: Tree rendering (DrawCmd generation)
- [ ] Phase 7: Direct tree rendering (optimization)
- [ ] Phase 8: Scrolling support
- [ ] Phase 9: DRM backend
- [ ] Phase 10: Unified backend selection
