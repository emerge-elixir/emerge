# EMRG Binary Format Specification

This document specifies the EMRG binary format used by `Emerge.Serialization.encode_tree/1`.

## Overview
EMRG is a compact, self-contained encoding of an `Emerge.Element` tree.
It stores all nodes in a flat list and reconstructs parent/child links by id.

## Header
```
"EMRG"            # 4 bytes ASCII magic
version           # 1 byte unsigned
node_count        # 4 bytes unsigned, big-endian
```

Current version is `3`.

## Node Record
Each node is encoded as:
```
id_len            # 4 bytes unsigned
id_bin            # id_len bytes (erlang term_to_binary)
type_tag          # 1 byte unsigned
attrs_len         # 4 bytes unsigned
attrs_bin         # attrs_len bytes (attribute block, see below)
child_count       # 2 bytes unsigned
children...       # repeated child ids (length-prefixed)
```

## Attribute Block
```
attr_count        # 2 bytes unsigned
attr_records...   # repeated attribute records
```

Encoding notes:

- Attribute records are sorted by tag during encoding.
- Runtime-only attributes are stripped before encoding.

### Attribute Record
```
attr_tag          # 1 byte unsigned
attr_value        # encoded per tag
```

### Child Id Record
Each child id is encoded as:
```
child_id_len      # 4 bytes unsigned
child_id_bin      # child_id_len bytes (erlang term_to_binary)
```

## Type Tags
```
row         -> 1
wrapped_row -> 2
column      -> 3
el          -> 4
text        -> 5
none        -> 6
paragraph   -> 7
text_column -> 8
image       -> 9
```

## Attribute Tags (current)
```
width             -> 1
height            -> 2
padding           -> 3
spacing           -> 4
align_x           -> 5
align_y           -> 6
scrollbar_y       -> 7
scrollbar_x       -> 8
clip_y            -> 10
clip_x            -> 11
background        -> 12
border_radius     -> 13
border_width      -> 14
border_color      -> 15
font_size         -> 16
font_color        -> 17
font             -> 18
font_weight       -> 19
font_style        -> 20
content           -> 21
above             -> 22
below             -> 23
on_left           -> 24
on_right          -> 25
in_front          -> 26
behind            -> 27
snap_layout       -> 28
snap_text_metrics -> 29
text_align        -> 30
move_x            -> 31
move_y            -> 32
rotate            -> 33
scale             -> 34
alpha             -> 35
spacing_xy        -> 36
space_evenly      -> 37
scroll_x          -> 38
scroll_y          -> 39
on_click          -> 40
on_mouse_down     -> 41
on_mouse_up       -> 42
on_mouse_enter    -> 43
on_mouse_leave    -> 44
on_mouse_move     -> 45
mouse_over        -> 46
font_underline    -> 47
font_strike       -> 48
font_letter_spacing -> 49
font_word_spacing -> 50
border_style      -> 51
box_shadow        -> 52
image_src         -> 53
image_fit         -> 54
image_size        -> 55
```

## Attribute Value Encodings (current)
### Length (`width`, `height`)
```
0 -> :fill
1 -> :content
2 -> {:px, f64}
3 -> {:fill_portion, f64}
4 -> {:minimum, f64, length}
5 -> {:maximum, f64, length}
```

`{:fill, n}` is encoded as variant `3` (same as `:fill_portion`).

### Padding (`padding`)
```
0 -> f64
1 -> {top, right, bottom, left} (4 x f64)
2 -> %{top, right, bottom, left} (4 x f64)
```

### Alignment (`align_x`, `align_y`)
```
align_x: 0 -> :left, 1 -> :center, 2 -> :right
align_y: 0 -> :top, 1 -> :center, 2 -> :bottom
```

### Text Alignment (`text_align`)
```
0 -> :left
1 -> :center
2 -> :right
```

### Booleans
```
0 -> false
1 -> true
```

### Numbers
```
f64 -> IEEE 754 float-64
```

### Colors (`font_color`, `border_color`, `background`)
```
0 -> {:color_rgb, {r,g,b}}  (3 x u8)
1 -> {:color_rgba, {r,g,b,a}} (4 x u8)
2 -> atom name (u16 len + bytes)
```

### Background
```
0 -> color (color encoding above)
1 -> {:gradient, from, to, angle} (color + color + f64)
2 -> {:image, source, fit} (image_source + image_fit)
```

### Border Style (`border_style`)
```
0 -> :solid
1 -> :dashed
2 -> :dotted
```

### Box Shadow (`box_shadow`)
```
u8 count
repeat count times:
  f64 offset_x
  f64 offset_y
  f64 blur
  f64 size
  color
  bool inset
```

### Image Source (`image_src`)
```
u8 variant + payload

variant 0 -> {:id, id}
  u16 len + bytes

variant 1 -> logical path
  u16 len + bytes

variant 2 -> {:path, path}
  u16 len + bytes
```

### Image Fit (`image_fit`)
```
0 -> :contain
1 -> :cover
2 -> :repeat
3 -> :repeat_x
4 -> :repeat_y
```

### Image Size (`image_size`)
```
f64 width + f64 height
```

### Border Radius (`border_radius`)
```
0 -> uniform radius (f64)
1 -> {tl, tr, br, bl} (4 x f64)
```

### Font
```
0 -> atom (u16 len + bytes)
1 -> binary string (u16 len + bytes)
```

### Atom-like values (`font_weight`, `font_style`)
```
u16 len + bytes
```

### Text Content
```
u16 len + bytes
```

### Nearby Elements (`above`, `below`, `on_left`, `on_right`, `in_front`, `behind`)
```
u32 len + EMRG subtree bytes
```

### Mouse Over (`mouse_over`)
```
u32 len + nested typed attr block
```

The nested block uses the same format as an attribute block (`attr_count` +
`attr_records`) but only allows decorative attrs:

- `background`
- `border_color`
- `font_color`
- `font_size`
- `font_underline`
- `font_strike`
- `font_letter_spacing`
- `font_word_spacing`
- `move_x`
- `move_y`
- `rotate`
- `scale`
- `alpha`

### Numeric attrs (`spacing`, `move_x`, `move_y`, `rotate`, `scale`, `alpha`, `scroll_x`, `scroll_y`, `font_letter_spacing`, `font_word_spacing`)
```
f64
```

### Font Decoration Bools (`font_underline`, `font_strike`)
```
0 -> false
1 -> true
```

### Spacing XY (`spacing_xy`)
```
f64 x + f64 y
```

### Boolean attrs
`scrollbar_*`, `clip_*`, `snap_*`, `space_evenly`, and event-presence attrs use:

```
0 -> false
1 -> true
```

For event attributes (`on_click`, `on_mouse_*`), Elixir encodes presence as
`true`.

`mouse_over` is not a presence flag; it stores the nested decorative attr block
defined above.

## Decoding
- All nodes are read into a map keyed by id.
- The first node in the stream is the root.
- Children are reconstructed by resolving ids in `child_ids`.
- Decoder accepts EMRG v3 payloads only.

## Notes
- Ids are serialized as Erlang terms.
- Attribute values use the typed encodings described above.
- The format is stable for the current version (`3`).

## v3 Image Changes
- Added `image` element type tag (`9`).
- Added image attrs: `image_src` (`53`), `image_fit` (`54`), `image_size` (`55`).
- Added typed image source variants (`id`, `logical`, `runtime_path`) for `image_src` and
  background image payloads.

## Internal / runtime (not encoded in EMRG)
- `:scroll_x`
- `:scroll_y`
- `:scroll_max`
- `:scroll_max_x`
- `:scroll_bounds`
- `:scroll_clip_bounds`
- `:clip_bounds`
- `:clip_content`
- `:text_baseline_offset`
- `:scroll_capture`
- `:mouse_over_active`
- `:__layer`
- `:nearby_behind`
- `:nearby_in_front`
- `:nearby_outside`
- `:__attrs_hash`

Rust also tracks `scroll_x_max` and `scroll_y_max` at runtime for clamping and
scrollbar behavior; these are computed fields and are not serialized by EMRG.
