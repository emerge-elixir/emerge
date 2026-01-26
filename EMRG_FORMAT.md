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
```

## Attribute Value Encodings (current)
### Length (`width`, `height`)
```
0 -> :fill
1 -> :content
2 -> {:px, f64}
3 -> {:fill_portion, f64}
```

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

### Font
```
0 -> atom (u16 len + bytes)
1 -> binary string (u16 len + bytes)
```

### Text Content
```
u16 len + bytes
```

### Nearby Elements (`above`, `below`, `on_left`, `on_right`, `in_front`, `behind`)
```
u32 len + EMRG subtree bytes
```

## Decoding
- All nodes are read into a map keyed by id.
- The first node in the stream is the root.
- Children are reconstructed by resolving ids in `child_ids`.

## Notes
- Ids are serialized as Erlang terms.
- Attribute values use the typed encodings described above.
- The format is stable for the current version (`2`).

## v2 Changes (for Rust)
- Attrs are no longer encoded with `term_to_binary`; use the typed attribute block.
- `attrs_bin` is now a compact list of tagged attribute records.
- Runtime-only attrs are excluded from encoding: `scroll_x`, `scroll_y`, `scroll_max`, `scroll_max_x`,
  `scroll_bounds`, `scroll_clip_bounds`, `clip_bounds`, `clip_content`, `text_baseline_offset`,
  `scroll_capture`, `__layer`, `__attrs_hash`, `nearby_*`.
- Attribute tags and value encodings are fixed in this spec (see above).

## Internal / runtime (not encoded in EMRG)
- `:scroll_y`
- `:scroll_x`
- `:__attrs_hash`
- `:text_baseline_offset`
- `:clip_bounds`
- `:clip_content`
- `:scroll_bounds`
- `:scroll_clip_bounds`
- `:scroll_max`
- `:scroll_max_x`
- `:scroll_capture`
- `:__layer`
