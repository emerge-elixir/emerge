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
| widthEach | ✅ | N/A | ✅ | Per-edge width with clip-based rendering |
| dashed/dotted | ✅ | N/A | ✅ | DashPathEffect-based rendering |
| shadow | ✅ | N/A | ✅ | MaskFilter blur, supports inset |
| glow | ✅ | N/A | ✅ | Sugar over shadow with zero offset |

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

- Background images
