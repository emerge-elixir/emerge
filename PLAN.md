# PLAN.md

Implementation plan for emerge_skia - a Skia renderer for Elixir with EMRG tree integration.

## Current State

Multi-backend Skia renderer with:
- Draw command decoding and rendering
- Wayland/X11 windowing
- Raster (offscreen CPU) backend
- Push-based input event delivery
- EventProcessor thread for hit testing, click/scroll dispatch, and redraws
- Scrollbar track/thumb hit testing, drag snapping, and axis-specific hover state
- Drag-scroll support with deadzone and finger-like direction
- Scroll state preserved across layout/patch with resize-aware clamping
- Clip- and rounded-corner-aware hit testing
- EMRG tree deserialization and patching
- Elixir-side tree definition + EMRG encoder
- Three-pass layout engine (scale + measurement + resolution)
- Scale factor support for high-DPI displays
- Tree-to-DrawCmd rendering

## Architecture

```
lib.rs (NIF entry, resources, registration)
    │
    ├── renderer.rs (DrawCmd, RenderState, font cache)
    │
    ├── backend/
    │   ├── wayland.rs (windowed)
    │   ├── drm.rs (direct KMS/DRM backend)
    │   └── raster.rs (offscreen CPU surface)
    │
    ├── input.rs (InputEvent + mask filter + encoder)
    ├── events.rs (EventProcessor, event registry, hit-test, event/scroll dispatch)
    │   └── events/scrollbar.rs (scrollbar interaction state machine + hit helpers)
    │
    └── tree/
        ├── mod.rs (public exports)
        ├── element.rs (Element with base_attrs/attrs, ElementTree, Frame)
        ├── attrs.rs (Attrs, Length, Color, Background, etc.)
        ├── deserialize.rs (EMRG binary parser)
        ├── patch.rs (incremental tree updates)
        ├── layout.rs (three-pass: scale → measure → resolve)
        ├── render.rs (ElementTree → Vec<DrawCmd>, reads pre-scaled attrs)
        ├── scrollbar.rs (scrollbar geometry/metrics shared by render + events)
        └── serialize.rs (ElementTree → EMRG binary)
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
- `{:cursor_pos, {x, y}}`
- `{:cursor_button, {button, action, mods, {x, y}}}`
- `{:cursor_scroll, {{dx, dy}, {x, y}}}`
- `{:key, {key, action, mods}}`
- `{:codepoint, {char, mods}}`
- `{:resized, {width, height, scale}}`
- `{:focused, bool}`
- `{:cursor_entered, entered}`

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
| 1 | width | Length: 0=fill, 1=content, 2=px+f64, 3=fill_portion+f64, 4=minimum, 5=maximum |
| 2 | height | Length (same as width) |
| 3 | padding | 0=uniform+f64, 1/2=sides+4×f64 |
| 4 | spacing | f64 |
| 5 | align_x | 0=left, 1=center, 2=right |
| 6 | align_y | 0=top, 1=center, 2=bottom |
| 7 | scrollbar_y | bool |
| 8 | scrollbar_x | bool |
| 10 | clip_y | bool |
| 11 | clip_x | bool |
| 12 | background | 0=color, 1=gradient(color+color+f64) |
| 13 | border_radius | 0=uniform+f64, 1=corners+4×f64 |
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
| 30 | text_align | 0=left, 1=center, 2=right |
| 31-35 | move_x/move_y/rotate/scale/alpha | f64 |
| 36 | spacing_xy | f64 x + f64 y |
| 37 | space_evenly | bool |
| 38 | scroll_x | f64 (runtime, typically stripped from Elixir encoding) |
| 39 | scroll_y | f64 (runtime, typically stripped from Elixir encoding) |
| 40-45 | on_click/on_mouse_* | bool (presence flag) |

Color encoding: 0=rgb(3×u8), 1=rgba(4×u8), 2=named(u16 len + bytes)

Runtime-only attrs are not encoded (`scroll_x`, `scroll_y`, `scroll_max`, `scroll_max_x`,
`scroll_bounds`, `scroll_clip_bounds`, `clip_bounds`, `clip_content`, `text_baseline_offset`,
`scroll_capture`, `__layer`, `nearby_behind`, `nearby_in_front`, `nearby_outside`, `__attrs_hash`).

#### 4.3 Tree NIF Functions

```elixir
tree_new()                              # Create empty tree resource
tree_upload(tree, binary)               # Upload EMRG binary, replaces contents
tree_patch(tree, binary)                # Apply incremental patches
tree_layout(tree, width, height, scale) # Compute layout with scale factor
tree_node_count(tree)                   # Get node count
tree_is_empty(tree)                     # Check if empty
tree_clear(tree)                        # Clear all nodes
```

#### 4.4 Patch Operations

| Tag | Operation | Format |
|-----|-----------|--------|
| 1 | SetAttrs | id_len + id + attr_len + attrs |
| 2 | SetChildren | id_len + id + count + child_ids |
| 3 | InsertSubtree | parent_len + parent_id + index + tree_len + tree_bytes |
| 4 | Remove | id_len + id |

### Phase 5: Layout Engine ✓

Three-pass layout algorithm in `layout.rs`:
1. **Scale pass**: Apply scale factor to all pixel-based attributes
2. **Measurement pass** (bottom-up): Compute intrinsic sizes for all elements
3. **Resolution pass** (top-down): Assign frames using constraints

Features:
- Fill distribution for rows/columns
- Alignment (align_x, align_y)
- Padding and spacing
- Wrapped row line breaking
- **Scale factor support** (for high-DPI displays)

#### Scaling Architecture

Each Element stores two copies of attributes:
- `base_attrs`: Original unscaled values (as received from Elixir)
- `attrs`: Scaled values (used by layout and render)

Scale is applied as Pass 0 before measurement, copying `base_attrs` → `attrs` with scaling:
- Width/height when using `px()` (including inside minimum/maximum)
- Padding (uniform and per-side)
- Spacing
- Border radius
- Border width
- Font size

This architecture ensures:
1. No cumulative scaling bugs (always scales from fresh `base_attrs`)
2. Patches update `base_attrs` with unscaled values; next layout rescales correctly
3. Render pass reads directly from pre-scaled `attrs` (no scaling logic needed)

Usage: `tree_layout(tree, width, height, scale)` where `scale > 1.0` for high-DPI.

Example: With `scale=2.0`, an element with `width(px(100))` becomes 200 physical pixels.

### Phase 6: Tree Rendering ✓

`render.rs` converts ElementTree to Vec<DrawCmd>:
- Background (solid color, gradient)
- Border (width, color, radius)
- Clipping (clip_x, clip_y)
- Text with font metrics

`serialize.rs` encodes ElementTree back to EMRG v2 binary format.

### Recent Updates (Post Phase 6)

- Per-corner border radius support (attrs, renderer, and draw commands)
- Transform attributes (move/rotate/scale) and alpha rendering
- Padding-aware clipping for clip_x/clip_y and scrollbar axes
- Text alignment implemented (align left/center/right)
- Length API expansion (shrink alias, minimum/maximum) with layout coverage
- Tests added for transforms, clipping, length encoding, and content sizing
- Added spacingXY + spaceEvenly (space-between) support
- Element event system implemented (`on_click` + `on_mouse_*`) with clip/rounded/scroll-aware hit testing
- EventProcessor introduced; input loop now enqueues raw events
- Click dispatch and hit testing moved to event processor
- Drag-scroll with deadzone; finger-like drag direction
- Directional scroll flags (per-axis can-scroll flags)
- Scroll state preserved through layout scaling/patches
- Resize handling: clamp scroll offsets when max shrinks; preserve start/end rules on grow
- Hit testing respects clip padding and rounded corners

---

## Elm-UI Feature Implementation

Goal: Implement elm-ui API one feature at a time until layout + rendering coverage is complete.

### Feature Status

#### Core Elements
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| text | ✅ | ✅ | ✅ | |
| el | ✅ | ✅ | ✅ | |
| row | ✅ | ✅ | ✅ | |
| column | ✅ | ✅ | ✅ | |
| wrappedRow | ✅ | ✅ | ✅ | |
| none | ✅ | ✅ | ✅ | |
| paragraph | ❌ | ❌ | ❌ | Inline text flow |
| textColumn | ❌ | ❌ | ❌ | Multi-paragraph |

#### Sizing
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| px | ✅ | ✅ | N/A | |
| fill | ✅ | ✅ | N/A | |
| fillPortion | ✅ | ✅ | N/A | |
| shrink/content | ✅ | ✅ | N/A | |
| minimum | ✅ | ✅ | N/A | **NEW** |
| maximum | ✅ | ✅ | N/A | **NEW** |

#### Spacing & Padding
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| padding (uniform) | ✅ | ✅ | N/A | |
| paddingXY | ✅ | ✅ | N/A | |
| paddingEach | ✅ | ✅ | N/A | |
| spacing | ✅ | ✅ | N/A | |
| spacingXY | ✅ | ✅ | N/A | Different H/V spacing |
| spaceEvenly | ✅ | ✅ | N/A | Space-between gaps |

#### Alignment
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| centerX | ✅ | ✅ | N/A | |
| centerY | ✅ | ✅ | N/A | |
| alignLeft | ✅ | ✅ | N/A | |
| alignRight | ✅ | ✅ | N/A | |
| alignTop | ✅ | ✅ | N/A | |
| alignBottom | ✅ | ✅ | N/A | |

#### Nearby Positioning
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| above | ✅ | ✅ | ✅ | Centered horizontally above |
| below | ✅ | ✅ | ✅ | Centered horizontally below |
| onLeft | ✅ | ✅ | ✅ | Centered vertically left |
| onRight | ✅ | ✅ | ✅ | Centered vertically right |
| inFront | ✅ | ✅ | ✅ | Centered overlay |
| behindContent | ✅ | ✅ | ✅ | Centered underlay |

#### Background
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| color | ✅ | N/A | ✅ | |
| gradient | ✅ | N/A | ✅ | Linear only |
| image | ❌ | N/A | ❌ | |
| tiled | ❌ | N/A | ❌ | |

#### Border
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| width | ✅ | N/A | ✅ | |
| color | ✅ | N/A | ✅ | |
| rounded (uniform) | ✅ | N/A | ✅ | |
| roundEach | ✅ | N/A | ✅ | Per-corner rendering |
| widthEach | ❌ | N/A | ❌ | Per-edge width |
| dashed/dotted | ❌ | N/A | ❌ | |
| shadow | ❌ | N/A | ❌ | |
| glow | ❌ | N/A | ❌ | |

#### Typography
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| Font.size | ✅ | ✅ | ✅ | |
| Font.color | ✅ | N/A | ✅ | |
| Font.family | ✅ | ✅ | ✅ | Family inheritance + fallback |
| Font.bold | ✅ | ✅ | ✅ | Weight mapping with synthetic fallback |
| Font.italic | ✅ | ✅ | ✅ | Italic mapping with synthetic fallback |
| Font.strike | ❌ | N/A | ❌ | |
| Font.underline | ❌ | N/A | ❌ | |
| Font.letterSpacing | ❌ | ❌ | ❌ | |
| Font.wordSpacing | ❌ | ❌ | ❌ | |
| Font.alignLeft/Right/Center | ✅ | ✅ | ✅ | Text alignment |

#### Transforms
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| move_x / move_y | ✅ | ✅ | ✅ | Equivalent to elm-ui move helpers |
| rotate | ✅ | ✅ | ✅ | |
| scale | ✅ | ✅ | ✅ | |

#### Effects
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| alpha/opacity | ✅ | N/A | ✅ | |

#### Clipping & Scrolling
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| clipX | ✅ | N/A | ✅ | |
| clipY | ✅ | N/A | ✅ | |
| scrollbarY | ✅ | ✅ | ✅ | Clips to padded content; updates scroll max |
| scrollbarX | ✅ | ✅ | ✅ | Clips to padded content; updates scroll max |

#### Input Elements
| Feature | Status | Notes |
|---------|--------|-------|
| Input.button | ❌ | |
| Input.checkbox | ❌ | |
| Input.text | ❌ | |
| Input.multiline | ❌ | |
| Input.slider | ❌ | |
| Input.radio | ❌ | |

---

## Implementation Roadmap

### Phase 7 - Per-Corner Border Radius ✓

Completed. `border_radius` now supports uniform + per-corner values, and rendering respects corner-specific radii.

### Phase 8 - Nearby Element Layout ✓

Completed. Nearby elements are decoded from EMRG bytes, laid out relative to parent, and rendered with correct z-ordering:
- `above`: Centered horizontally, positioned above parent
- `below`: Centered horizontally, positioned below parent
- `on_left`: Centered vertically, positioned left of parent
- `on_right`: Centered vertically, positioned right of parent
- `in_front`: Centered overlay (rendered last)
- `behind`: Centered underlay (rendered first)

### Phase 9 - Transforms ✓

Completed.

- Transform attrs are encoded/decoded: `move_x`, `move_y`, `rotate`, `scale`.
- Render pipeline applies translate/rotate/scale around element center.
- `alpha` opacity is supported via layer alpha.

### Phase 10 - Font Rendering Improvements (Partial)

Completed:

1. Load font families by name (`Font.family`)
2. Apply font weight (`Font.bold`)
3. Apply font style (`Font.italic`)

Remaining:

1. Text decoration (`underline`, `strike`)
2. Letter/word spacing controls

### Phase 11 - Scrollbars ✓

Completed:

1. Thumb rendering and position/size from scroll state
2. Wheel + content drag-scroll input with deadzone
3. Scrollbar track/thumb hit testing
4. Thumb drag behavior with snap-to-cursor track press
5. Axis-specific scrollbar hover state (`none | x | y`) with hover width styling

Optional follow-ups:

1. Distinct pressed/active thumb visual treatment

### Phase 12 - Advanced Features

- `paragraph` element for inline text flow
- `textColumn` for multi-paragraph layouts
- Border shadows and glow effects
- Background images

---

## Possible Improvements to Layout Engine

Based on analysis of [taffy](https://github.com/DioxusLabs/taffy) layout engine. Focus is on elm-ui semantics, not CSS compatibility.

### High Priority

#### 1. FillPortion Proper Distribution ✓

Implemented. `FillPortion(n)` now distributes space proportionally:
- `fill` is equivalent to `fillPortion(1)`
- Children with `fillPortion(2)` get twice the space of `fillPortion(1)`
- Fixed-size children are subtracted first, remaining space distributed by portions

#### 2. Layout Caching
Taffy uses a 9-slot cache per node to avoid redundant calculations. Our implementation recalculates everything on every layout pass.

```rust
pub struct LayoutCache {
    cached_size: Option<(Option<f32>, Option<f32>, IntrinsicSize)>,
    generation: u32,  // Invalidation counter
}
```

Benefit: Significant performance improvement for incremental updates.

### Medium Priority

#### 3. Pixel Rounding Pass
Taffy rounds based on cumulative coordinates to prevent pixel gaps:

```rust
fn round_layout(tree: &mut ElementTree) {
    for element in tree.nodes.values_mut() {
        if let Some(frame) = &mut element.frame {
            // Round edges, not dimensions (prevents gaps)
            let left = frame.x.round();
            let top = frame.y.round();
            let right = (frame.x + frame.width).round();
            let bottom = (frame.y + frame.height).round();
            frame.x = left;
            frame.y = top;
            frame.width = right - left;
            frame.height = bottom - top;
        }
    }
}
```

Benefit: Crisp pixel-perfect rendering, eliminates hairline gaps.

#### 4. Richer Constraint Type ✓

Implemented. `AvailableSpace` enum provides more expressive constraints:

```rust
pub enum AvailableSpace {
    Definite(f32),    // Fixed constraint (px or fill resolved)
    MinContent,       // Shrink to minimum
    MaxContent,       // Expand to content
}

pub struct Constraint {
    pub width: AvailableSpace,
    pub height: AvailableSpace,
}
```

- `Constraint::new(w, h)` creates definite constraints (most common)
- `Constraint::with_space()` allows content-based constraints
- MinContent/MaxContent resolve to intrinsic sizes during layout
- MaxContent is now used to gate fill distribution when content-sized

#### 5. Events System ✓

Implemented. Event processing now runs in `events.rs` via `EventProcessor`:
- Registry built post-layout with clip + rounded-corner hit testing
- Click detection on press/release with `{:emerge_skia_event, {element_id, :click}}`
- Scroll handling via directional flags (per-axis can-scroll)
- Drag-scroll with deadzone and finger-like direction
- Scrollbar module extraction (`events/scrollbar.rs`) for typed scrollbar hit/drag state
- Track/thumb hit testing + thumb drag with snap-to-cursor behavior
- Input loop enqueues raw events; processor handles dispatch + redraw

#### 5. Content Size Tracking ✓

Implemented. Frame now tracks actual content extent separately from visible size:

```rust
pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,          // Visible frame size
    pub height: f32,
    pub content_width: f32,  // Actual content extent (for scroll calculations)
    pub content_height: f32,
}
```

- For elements with children, content_width/content_height reflects actual child layout
- For scrollable elements (clip/scrollbar), frame stays fixed while content tracks overflow
- For empty containers, content equals frame size

### Scroll Behavior Rules

- Scroll state is runtime-only and preserved across layout/patch.
- When scroll max shrinks, offsets clamp toward the start (0).
- When scroll max grows, end-anchoring applies only if the previous offset was at end; otherwise the offset is preserved and clamped.
- When scrollbar_* is disabled, scroll_* and scroll_*_max are cleared.

### Low Priority

#### 6. Separate Size Computation Mode
Taffy has `RunMode::ComputeSize` for measuring without full layout:

```rust
pub enum LayoutMode {
    FullLayout,      // Compute frames
    MeasureOnly,     // Just compute sizes (faster)
}
```

Benefit: Faster intrinsic size queries without side effects.

### Not Recommended (Keep elm-ui Focus)

These CSS features from taffy should **not** be added:
- ❌ CSS Grid (elm-ui uses row/column composition)
- ❌ Absolute/relative positioning (elm-ui uses above/below/inFront/behind)
- ❌ Margin collapse (elm-ui uses spacing)
- ❌ Flexbox shrink factors (elm-ui uses simpler model)
- ❌ Complex percentage resolution (elm-ui percentages are rare)
- ❌ Aspect ratio constraints (not in elm-ui)

---

## Length Encoding Reference

| Variant | Tag | Format |
|---------|-----|--------|
| fill | 0 | (no data) |
| content | 1 | (no data) |
| px | 2 | f64 |
| fill_portion | 3 | f64 |
| minimum | 4 | f64 + inner length |
| maximum | 5 | f64 + inner length |
