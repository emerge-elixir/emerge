# Architecture & Feature Status

EmergeSkia is a Skia renderer for Elixir with EMRG tree integration.

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
- Declarative `mouse_over` styling with runtime active-state application
- EMRG tree deserialization and patching
- Elixir-side tree definition + EMRG encoder
- Three-pass layout engine (scale + measurement + resolution)
- Scale factor support for high-DPI displays
- Tree-to-DrawCmd rendering

## Module Structure

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
| paragraph | ✅ | ✅ | ✅ | Inline text flow |
| textColumn | ✅ | ✅ | ✅ | Multi-paragraph (column semantics for now) |

#### Sizing
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| px | ✅ | ✅ | N/A | |
| fill | ✅ | ✅ | N/A | |
| fillPortion | ✅ | ✅ | N/A | |
| shrink/content | ✅ | ✅ | N/A | |
| minimum | ✅ | ✅ | N/A | |
| maximum | ✅ | ✅ | N/A | |

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
| Font.strike | ✅ | ✅ | ✅ | Inherited + mouse_over compatible |
| Font.underline | ✅ | ✅ | ✅ | Inherited + mouse_over compatible |
| Font.letterSpacing | ✅ | ✅ | ✅ | Inherited + mouse_over compatible |
| Font.wordSpacing | ✅ | ✅ | ✅ | Inherited + mouse_over compatible |
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

## Scaling Architecture

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
- Font letter/word spacing

This architecture ensures:
1. No cumulative scaling bugs (always scales from fresh `base_attrs`)
2. Patches update `base_attrs` with unscaled values; next layout rescales correctly
3. Render pass reads directly from pre-scaled `attrs` (no scaling logic needed)

Usage: `tree_layout(tree, width, height, scale)` where `scale > 1.0` for high-DPI.

## Possible Layout Engine Improvements

Based on analysis of [taffy](https://github.com/DioxusLabs/taffy) layout engine. Focus is on elm-ui semantics, not CSS compatibility.

### Layout Caching
Taffy uses a 9-slot cache per node to avoid redundant calculations. Our implementation recalculates everything on every layout pass.

```rust
pub struct LayoutCache {
    cached_size: Option<(Option<f32>, Option<f32>, IntrinsicSize)>,
    generation: u32,  // Invalidation counter
}
```

Benefit: Significant performance improvement for incremental updates.

### Pixel Rounding Pass
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

### Separate Size Computation Mode
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
- CSS Grid (elm-ui uses row/column composition)
- Absolute/relative positioning (elm-ui uses above/below/inFront/behind)
- Margin collapse (elm-ui uses spacing)
- Flexbox shrink factors (elm-ui uses simpler model)
- Complex percentage resolution (elm-ui percentages are rare)
- Aspect ratio constraints (not in elm-ui)

## Upcoming Features

- Advanced `textColumn` float-style wrapping around `alignLeft`/`alignRight` blocks
- Border shadows and glow effects
- Background images

## EMRG Attribute Reference

See the [EMRG Format](emrg-format.md) guide for the full binary encoding specification.

### Length Encoding

| Variant | Tag | Format |
|---------|-----|--------|
| fill | 0 | (no data) |
| content | 1 | (no data) |
| px | 2 | f64 |
| fill_portion | 3 | f64 |
| minimum | 4 | f64 + inner length |
| maximum | 5 | f64 + inner length |
