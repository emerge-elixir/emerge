# EMRG Binary Format Specification

This document specifies the EMRG binary format used by
`Emerge.Engine.Serialization.encode_tree/1`.

## Overview

EMRG is a compact, self-contained encoding of a retained `Emerge.tree()` value.
It stores all nodes in a flat list and rebuilds edges by id.

Current format version is `6`.

## Header

```text
"EMRG"            # 4 bytes ASCII magic
version           # 1 byte unsigned
node_count        # 4 bytes unsigned, big-endian
```

## Node Record

Each node is encoded as:

```text
id_len            # 4 bytes unsigned
id_bin            # id_len bytes (erlang term_to_binary)
type_tag          # 1 byte unsigned
attrs_len         # 4 bytes unsigned
attrs_bin         # attrs_len bytes (attribute block)
child_count       # 2 bytes unsigned
children...       # repeated child ids (length-prefixed)
nearby_count      # 2 bytes unsigned
nearby...         # repeated mounted nearby refs: slot_tag + length-prefixed id
```

Nearby slot tags use the following fixed mapping:

1. `behind_content`
2. `above`
3. `on_right`
4. `below`
5. `on_left`
6. `in_front`

Only present mounts are encoded.

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
```

## Attribute Tags

The attr block continues to use the typed attr encoding from
`Emerge.Engine.AttrCodec`, but nearby tags are no longer present in v6.

Notable tag coverage includes:

```text
width, height, padding, spacing, spacing_xy
align_x, align_y, text_align
scrollbar_x, scrollbar_y, scroll_x, scroll_y
background, border_radius, border_width, border_style, border_color, box_shadow
font, font_size, font_color, font_weight, font_style
font_underline, font_strike, font_letter_spacing, font_word_spacing
content
move_x, move_y, rotate, scale, alpha
image_src, image_fit, image_size
video_target
on_click, on_press, on_mouse_down, on_mouse_up, on_mouse_enter,
on_mouse_leave, on_mouse_move, on_change, on_focus, on_blur
mouse_over, focused, mouse_down
snap_layout, snap_text_metrics, space_evenly
```

## Compatibility Note

Rust still accepts EMRG v5 and early v3 during decode for compatibility with
older tests, but all new Elixir serialization uses v6.
