# EMRG Binary Format Specification

`Emerge.Engine.Patch` and `Emerge.Engine.Serialization` use the same format.

## Overview

EMRG v10 encodes **`patch(target, subtree)`**. The body contains its root ID and
zero or more shallow definitions. A fully declared first mount targets the
vacant viewport (`0`). An empty binary means no tree change, not clear.

## Header

```text
"EMRG"            # 4 bytes ASCII magic
10                # one byte; all multibyte fields below are big-endian
target            # u64 occurrence being replaced, 0 only when vacant
body_root         # u64; 0 = empty, no records; a bare live ID can be promoted
record_count      # u32; counts definitions, not references or live nodes
```

## Node Record

Each node is encoded as:

```text
id                # 8 bytes unsigned, big-endian
type_tag          # 1 byte unsigned
fields            # u8; bit 0: replace attrs, bit 1: replace edges; others zero
if fields & 1:
  attrs_len       # u32
  attrs_bin       # entire attribute block; a zero attribute count clears it
if fields & 2:
  child_count     # u16
  children...     # ordered ordinary child IDs, u64 each
  nearby_count    # u16
  nearby...       # ordered mounted Nearby refs: slot_tag + u64 id
```

Nearby slot tags use the following fixed mapping:

1. `behind_content`
2. `above`
3. `on_right`
4. `below`
5. `on_left`
6. `in_front`

With a field bit clear, that field is `:same`: no length, count or payload is
sent for it. Fresh occurrences must set both bits. A reachable ID without a
record keeps its entire live subtree from the last **accepted** declaration.
Only that reference or a kept edge list closes a branch; kept attributes do
not. IDs must be positive, uniquely owned and inside the target if already
live. Definitions can precede or follow their parents. Extra definitions,
cycles, double owners, outside or detached captures are invalid. Nearby is
representable in Elixir; native support is limited to the ported shapes/edges.

## Attribute Block

The attr block still contains typed attribute records, but it now contains only
ordinary attrs.

- runtime attrs are stripped before encoding
- nearby attrs are stripped before encoding
- nearby structure is carried by node-level mount refs instead

## Type Tags

```text
row         -> 1
wrapped_row -> 2
column      -> 3
el          -> 4
text        -> 5
none        -> 6
paragraph   -> 7
text_column -> 8
image       -> 9
text_input  -> 10
video       -> 11
multiline   -> 12
slider      -> 13
wrapped_column -> 14
```

Tag 14 is an addition to v10. The header, the first thirteen tags, child
references and attribute encoding are unchanged. Older retained readers
reject the new kind, not the whole format. Paragraph and TextColumn use the
existing tags `7` and `8` and ordinary declared child order. No line fragments,
float offsets, glyph ranges or continuation hit boxes cross the wire.

## Attribute Tags

The attr block continues to use the typed attr encoding from
`Emerge.Engine.AttrCodec`, but nearby tags are no longer present in v6.

Notable tag coverage includes:

```text
width, height, layout_scale, layout_rotate, padding, spacing, spacing_xy
align_x, align_y, text_align
scrollbar_x, scrollbar_y, scroll_x, scroll_y
background, border_radius, border_width, border_style, border_color, box_shadow
font, font_size, font_color, font_weight, font_style
font_underline, font_strike, font_letter_spacing, font_word_spacing
content
move_x, move_y, rotate, scale, alpha
animate, animate_enter, animate_exit, animate_change
image_src, image_fit, image_size
video_target
on_click, on_press, on_mouse_down, on_mouse_up, on_mouse_enter,
on_mouse_leave, on_mouse_move, on_change, on_focus, on_blur
mouse_over, focused, mouse_down
snap_layout, snap_text_metrics, space_evenly
```

Tag `30` (`text_align`) takes one byte: `0` left, `1` center, `2` right.
The declaration inherits when absent; explicit `0` overrides an inherited
center or right. Only authored values are serialized, not the inherited
alignment, offered width or glyph origin. Tags `5` and `6` normally position
boxes. A paragraph also reads its own `5` as a fallback for line alignment;
explicit left/right on a child of Paragraph or TextColumn selects a float.
TextColumn shares exclusions across its direct paragraphs. Omission is not an
explicit left float. Unknown or missing alignment bytes are refused.

Tag `37` (`space_evenly`) carries one byte: `0` for false, `1` for true.
All supported kinds retain the declared bytes, including explicit false.
Only Row, Column, WrappedRow and WrappedColumn use the flag; it has no
layout effect on El, Text, None, Paragraph or TextColumn. Calculated gaps are
never serialized. A missing or non-boolean payload is refused before replacing
the accepted tree.

Layout scale is tag `78` followed by a finite, positive big-endian `f64`.
The wire contains the authored factor, never the product with an ancestor or
the display. An attrs-only replacement can rescale a closed subtree without
resending child IDs. Paint transform scale (`34`) is a different attribute.

## Background and Gradient Payloads

Attribute blocks start with a `u16` big-endian attribute count. Each attribute
starts with a one-byte tag; background uses tag `12` followed by:

| Variant | Payload |
| --- | --- |
| `0` | color (solid or gradient) |
| `1` | retired; rejected |
| `2` | image source and fit (unchanged) |
| `3` | retired background-only gradient; rejected |

Solid colors use variant `0` followed by three RGB bytes, variant `1` followed by
four RGBA bytes, or variant `2` followed by a `u16` BE UTF-8 name length and name.
Color variant `3` contains a `u32` BE stop count, ordered solid-color payloads,
and a finite `f64` BE angle in degrees. Gradients contain at least two solid
colors, never nested gradients. All color slots use this encoding: backgrounds,
fonts, borders, shadows and SVG tint. Background gradients therefore begin with
background variant `0`, then color variant `3`. Stops are evenly spaced.

Gradient values decode in Elixir as `{:color_gradient, colors, angle}`. Attribute
blocks (including keyframe blocks) must be consumed completely. Bad counts,
truncation, retired variants, and non-finite gradient angles are rejected.

## Animation Policies

Regular, enter and exit specs use tags `65`, `66` and `67`. Animated width/height
endpoints accept pixels, content, fill and weighted fill in any pairing. `min` and
`max` remain supported as static layout lengths, but are rejected in animation
keyframes and change policies. Other animated properties retain their existing
shape-compatibility checks.

Controlled-scroll attributes use new tags; legacy pixel-offset tags `38`/`39`
retain their old encoding and are not reinterpreted as fractions:

- `85`: `scroll_position_x`
- `86`: `scroll_position_y`
- `87`: `on_scroll` event presence (boolean)

Position payloads are `0:u8 + fraction:f64be` (finite, in `[0, 1]`) or
`1:u8 + declaration:u64be + target_node_id:u64be`. Zero target ID denotes an
unresolved key. Semantic Elixir key terms never cross the wire. Declaration
identity is independent of target-node resolution; native patch ingestion assigns
mount-scoped per-axis proposal sequence IDs only when the declaration changes.

Tag `84` encodes `animate_change`, separate from the ordinary target attributes:

```text
payload_len       # u32 BE byte length of the following payload
policy_count      # u16 BE
policies...       # field: u16 BE UTF-8 byte length + bytes
                  # duration_ms: f64 BE, finite and positive
                  # curve: u16 BE UTF-8 byte length + bytes
```

Curves are `linear`, `ease_in`, `ease_out`, or `ease_in_out`. Fields use their
ordinary attribute names. Spacing and spacing_xy refer to the same native field.
Targets remain in the surrounding attribute block; no source geometry, clocks,
groups, or history are serialized. Duplicate/conflicting policies, missing targets,
regular-animation ownership conflicts, and trailing payload data are rejected.
An absent policy removes it. First mount does not start an implicit change run.

## Compatibility Note

The ordinary binary CPU headless producer accepts v10 only. Upgrade its Elixir
writer and native library together. Re-encode persisted declarations; changing
only a version byte is insufficient. There is no opcode translator or upload
fallback. Neither the Elixir branch/site stamp nor a callback address crosses
this wire. The detached round-trip encoder writes a fully explicit declaration;
raw-attribute round trips alone do not qualify geometry, paint or input.
The legacy window hosts are not qualified v10 consumers.
