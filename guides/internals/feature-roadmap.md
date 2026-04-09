# Feature Roadmap

Feature implementation status and future plans for EmergeSkia.

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
| textColumn | ✅ | ✅ | ✅ | Multi-paragraph with float flow around alignLeft/alignRight |
| image | ✅ | ✅ | ✅ | Async Rust-loaded image element |

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
| above | ✅ | ✅ | ✅ | Left-start above host; `width fill` uses host width |
| below | ✅ | ✅ | ✅ | Left-start below host; `width fill` uses host width |
| onLeft | ✅ | ✅ | ✅ | Top-start left of host; `height fill` uses host height |
| onRight | ✅ | ✅ | ✅ | Top-start right of host; `height fill` uses host height |
| inFront | ✅ | ✅ | ✅ | Border-box slot; fill uses both axes; explicit size may overflow |
| behindContent | ✅ | ✅ | ✅ | Border-box slot between background and content |

#### Background
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| color | ✅ | N/A | ✅ | |
| gradient | ✅ | N/A | ✅ | Linear only |
| image | ✅ | N/A | ✅ | Default `:cover`; supports contain/cover/repeat modes |
| uncropped | ✅ | N/A | ✅ | `:contain` background helper |
| tiled | ✅ | N/A | ✅ | Repeat on both axes |
| tiledX | ✅ | N/A | ✅ | Repeat on X axis only |
| tiledY | ✅ | N/A | ✅ | Repeat on Y axis only |

#### Border
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| width | ✅ | N/A | ✅ | Interaction-style compatible |
| color | ✅ | N/A | ✅ | Interaction-style compatible |
| rounded (uniform) | ✅ | N/A | ✅ | Interaction-style compatible |
| roundEach | ✅ | N/A | ✅ | Per-corner rendering + interaction-style compatible |
| widthEach | ✅ | N/A | ✅ | Per-edge width with clip-based rendering + interaction-style compatible |
| dashed/dotted | ✅ | N/A | ✅ | DashPathEffect-based rendering + interaction-style compatible |
| shadow | ✅ | N/A | ✅ | MaskFilter blur, supports inset; interaction-style compatible |
| glow | ✅ | N/A | ✅ | Sugar over shadow with zero offset; interaction-style compatible |

#### Typography
| Feature | Elixir API | Layout | Render | Notes |
|---------|------------|--------|--------|-------|
| Font.size | ✅ | ✅ | ✅ | Inherited + interaction-style compatible |
| Font.color | ✅ | N/A | ✅ | Inherited + interaction-style compatible |
| Font.family | ✅ | ✅ | ✅ | Family inheritance + fallback; interaction-style compatible |
| Font.bold | ✅ | ✅ | ✅ | Weight mapping with synthetic fallback; interaction-style compatible |
| Font.italic | ✅ | ✅ | ✅ | Italic mapping with synthetic fallback; interaction-style compatible |
| Font.strike | ✅ | ✅ | ✅ | Inherited + interaction-style compatible |
| Font.underline | ✅ | ✅ | ✅ | Inherited + interaction-style compatible |
| Font.letterSpacing | ✅ | ✅ | ✅ | Inherited + interaction-style compatible |
| Font.wordSpacing | ✅ | ✅ | ✅ | Inherited + interaction-style compatible |
| Font.alignLeft/Right/Center | ✅ | ✅ | ✅ | Text alignment + interaction-style compatible |

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
| Input.button | ✅ | |
| Input.checkbox | ❌ | |
| Input.text | ✅ | Single-line controlled text input |
| Input.multiline | ✅ | Wrapped multiline input with auto-grow and multiline editing |
| Input.slider | ❌ | |
| Input.radio | ❌ | |

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

- Input primitives (button, checkbox, text, slider, etc.)
