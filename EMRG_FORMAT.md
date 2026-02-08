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

Current version is `2`.

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

### Numeric attrs (`spacing`, `move_x`, `move_y`, `rotate`, `scale`, `alpha`, `scroll_x`, `scroll_y`)
```
f64
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

## Decoding
- All nodes are read into a map keyed by id.
- The first node in the stream is the root.
- Children are reconstructed by resolving ids in `child_ids`.
- Decoder accepts v1 and v2 payloads; v2 uses typed attribute blocks.

## Notes
- Ids are serialized as Erlang terms.
- Attribute values use the typed encodings described above.
- The format is stable for the current version (`2`).

## v2 Changes (for Rust)
- Attrs are no longer encoded with `term_to_binary`; use the typed attribute block.
- `attrs_bin` is now a compact list of tagged attribute records.
- Runtime-only attrs are excluded from encoding: `scroll_x`, `scroll_y`, `scroll_max`,
  `scroll_max_x`, `scroll_bounds`, `scroll_clip_bounds`, `clip_bounds`, `clip_content`,
  `text_baseline_offset`, `scroll_capture`, `__layer`, `__attrs_hash`, and `nearby_*`.
- Attribute tags and value encodings are fixed in this spec (see above).

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
- `:__layer`
- `:nearby_behind`
- `:nearby_in_front`
- `:nearby_outside`
- `:__attrs_hash`

Rust also tracks `scroll_x_max` and `scroll_y_max` at runtime for clamping and
scrollbar behavior; these are computed fields and are not serialized by EMRG.
