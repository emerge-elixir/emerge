# Retained renderer: elements and attributes still to port

**Coverage through [static width/height expressions](dimension-regressions.md), EMRG v10.** The Elixir producer can
encode 13 element kinds; the active native renderer implements six. This is a
checklist of rendering and input behavior, not just of encodable bytes. An
unchecked item may already have a DSL helper, patch codec or legacy renderer.

## Elements

| Implemented kind (wire tag) | Current boundary |
| --- | --- |
| `row` (1) | Single horizontal line; widths share the remaining space by weight, redistributing at pixel or content limits. |
| `wrapped_row` (2) | Wraps at the offered width. Each line advances by its tallest **outer** child plus the vertical gap. |
| `column` (3) | Vertical stack. A fixed or allocated height redistributes the remainder among fills not stopped by a cap or floor; a content-height stack uses their individual intrinsic heights. Like both row kinds, clips ordinary descendants' paint and hits to its frame. |
| `el` (4) | Single-child wrapper in the DSL. `Input.button/2` builds an `el`, not a new kind; it can use `on_click`, but `on_press` is not ported. |
| `text` (5) | Single-line UTF-8 `content`; CR/LF is rejected. Not paragraph flow or an editable field. |
| `none` (6) | Empty leaf, not an absent tree or a removal. |

- [ ] `paragraph` (7) — inline word wrapping around floats, line alignment and decorations.
- [ ] `text_column` (8) — stack paragraph-oriented content with fill-width/content-height defaults.
- [ ] `image` (9) — source loading, intrinsic size, fitting and painting.
  `svg/2` is this same kind with `svg_expected`, not a fourteenth kind.
- [ ] `text_input` (10), `Input.text/2` — editing, caret, selection, focus and IME.
- [ ] `video` (11) — fit and present frames submitted to the named target.
- [ ] `multiline` (12), `Input.multiline/2` — wrapped editing, auto-height and scrolling.
- [ ] `slider` (13), `Input.slider/2` — controlled value, track/filled track/thumb and input.
- [ ] **Nearby mounts** — `behind`, `above`, `on_right`, `below`, `on_left`,
  `in_front`. These are ordered **edges**, not element kinds or ordinary attrs.
  Reconciliation understands them; native admission rejects nonempty Nearby lists.

## Attributes

**Working on the supported shapes:** `padding` (uniform, XY and per-edge),
`spacing`, `spacing_xy`, `layout_scale` (`Emerge.UI.scale/1`), `font_size` and
`on_click` (left press/release with element-local coordinates). An implicit
font size is 16 logical pixels at the text site; an explicit size resolves
where declared. `key/1` is supported occurrence identity, not a wire attr.

### Partially implemented — keep the working forms

| Attribute | Works now | Still to port |
| --- | --- | --- |
| `width`, `height` | `px`, `content`/`shrink`, and expressions over just these leaves on either axis. Plain `width(fill())` keeps its existing offers. [Weighted and compound fill](dimension-regressions.md#coverage-matrix): width on a root Row, direct Row child (shared), or [direct Column child (independent)](column-cross-axis-width.md#results); any fill-bearing height on a root Column or direct Column child. Both orders of `min`/`max`, including [mixed content/fill](../test/emerge_skia/mixed_content_fill_test.exs), repeated leaves and multiple weights, up to 64 operator levels. | Other weighted/compound width sites (including root Column, El and WrappedRow parents), cross-axis heights and WrappedRow distribution. Animation lengths are separate. |
| `align_x`, `align_y`, `text_align` | Leading values: left, top, left respectively | Horizontal center/right, vertical center/bottom, centered/right-aligned text. |
| `background`, `font_color` | Solid RGB/RGBA; raw wire names `black`, `white`, `red`, `blue`, `transparent`. Palette helpers returning RGB/RGBA work as solids. | Other raw names, gradients; for `background`, image fits and tiling. |
| `font` | `default` and `sans-serif` use bundled Inter Regular; `monospace` uses bundled JetBrains Mono Regular. | Other families and registered/custom fonts. |
| `content` | Single-line `text` payload | Multiline text and the content semantics of editable fields. |

### Not implemented — grouped by behavior

- [ ] **Layout distribution and snapping:** `space_evenly`, `snap_layout`, `snap_text_metrics`.
- [ ] **Layout rotation:** `layout_rotate` (`Emerge.UI.rotate/1`, tag 79).
- [ ] **Paint transforms and opacity:** `move_x`, `move_y`, `rotate`, `scale`,
  `alpha`. Paint `Transform.scale/1` (tag 34) is distinct from working
  `layout_scale` (tag 78). Transformed listeners need inverse-mapped coordinates.
- [ ] **Borders and shadows:** `border_radius`, `border_width`, `border_color`,
  `border_style`, `box_shadow` (including inner shadows and glows).
- [ ] **Typography:** `font_weight`, `font_style`, `font_underline`, `font_strike`,
  `font_letter_spacing`, `font_word_spacing`. Drawing the default regular face
  does not mean an explicit `font_weight` attribute is accepted.
- [ ] **Media:** `image_src`, `image_fit`, `image_size`, `svg_expected`, `svg_color`,
  `video_target`. Some are constructor-owned fields, not public attrs to set by hand.
- [ ] **Nearby clipping:** `clip_nearby`. Ordinary Column/Row clipping does not
  implement host-local or escaping overlay behavior.
- [ ] **Scrolling:** `scrollbar_x`, `scrollbar_y`, `scroll_position_x`,
  `scroll_position_y`, `on_scroll`. The producer resolves keyed scroll targets;
  that does not scroll a retained node. Codec names `scroll_x`/`scroll_y` are
  runtime offsets stripped from declarations, not the public scroll commands.
- [ ] **Mouse and activation listeners:** `on_mouse_down`, `on_mouse_up`,
  `on_mouse_move`, `on_mouse_enter`, `on_mouse_leave`, `on_press`.
- [ ] **Gestures:** `on_drag`, `on_swipe_up`, `on_swipe_down`, `on_swipe_left`,
  `on_swipe_right`, including capture and logical local endpoints.
- [ ] **Keys and focus:** `on_key_down`, `on_key_up`, `on_key_press`, `virtual_key`,
  `focus_on_mount`, `on_focus`, `on_blur`. Raw input in `handle_input/2` is not
  an element listener or native editing support.
- [ ] **Controls:** `on_change`, `slider_min`, `slider_max`, `slider_value`,
  `slider_step`. `Slider.config/1` is expanded on the Elixir side.
- [ ] **State styles:** `mouse_over`, `mouse_down`, `focused`.
- [ ] **Animation:** `animate`, `animate_enter`, `animate_exit`, `animate_change`.

## How to close an item

Check the [producer kind list](../lib/emerge/engine/patch.ex) and
[attribute tags](../lib/emerge/engine/attr_codec.ex) against the active
[shape dispatch](../native/emerge_skia/src/tree/shapes.rs),
[attribute decoder](../native/emerge_skia/src/tree/wire.rs) and
[per-shape payloads](../native/emerge_skia/src/tree/shapes/payload.rs).
When porting a variant, move **only that variant** to the working column and
link its regression. If `main` itself is wrong, record the example and its
status in the [changelog candidates](retained-changelog-candidates.md) rather
than treating parity as a fix. Useful starting cases:

- [Paragraph alignment](../test/emerge_skia/paragraph_alignment_test.exs), [gradient consumers](../test/emerge_skia/gradient_test.exs).
- [Controlled scroll](../test/emerge/controlled_scroll_test.exs), [mouse coordinates through scroll and transforms](../test/emerge/mouse_coordinates_test.exs), [drag capture](../test/emerge/pointer_drag_test.exs), [animated hover](../test/emerge_skia/live_animation_hover_test.exs).

Exercise a cold mount and a sparse edit on an ordinary viewport. For visual
changes check the delivered pixels; for input send raw events and check the
application callback against the **submitted** frame. A window claim needs
compositor pixels and seat events. A codec round trip, compiled backend or
skipped hardware test is not evidence that an item is implemented.
