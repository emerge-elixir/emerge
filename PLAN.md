# PLAN.md

Implementation plan for emerge_skia - a Skia renderer for Elixir with EMRG tree integration.

## Current State

Multi-backend Skia renderer with:
- Draw command decoding and rendering
- Wayland/X11 windowing
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
    │   ├── wayland.rs (windowed)
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

## Notes

- Removed unused helpers to keep the codebase warning-free: `Constraint::unbounded`, `ElementKind::to_tag`,
  `ElementTree::remove`, `ElementTree::children`, and the unused `MeasuredElement` stub.
- Dropped module-wide re-exports in `tree/mod.rs`; consumers should import from submodules
  (e.g. `tree::layout::Constraint`, `tree::patch::decode_patches`).

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
