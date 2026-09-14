# EMRG Binary Format Specification

This document specifies the EMRG binary format used by
`Emerge.Engine.Serialization.encode_tree/1`.

## Overview

EMRG is a compact, self-contained encoding of a retained `Emerge.tree()` value.
It stores all nodes in a flat list and rebuilds edges by id.

Current format version is `9`.

## Header

```text
"EMRG"            # 4 bytes ASCII magic
version           # 1 byte unsigned
node_count        # 4 bytes unsigned, big-endian
```

## Node Record

Each node is encoded as:

```text
id                # 8 bytes unsigned, big-endian
type_tag          # 1 byte unsigned
attrs_len         # 4 bytes unsigned
attrs_bin         # attrs_len bytes (attribute block)
child_count       # 2 bytes unsigned
children...       # repeated child ids (8 bytes each)
nearby_count      # 2 bytes unsigned
nearby...         # repeated mounted nearby refs: slot_tag + 8-byte id
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
slider      -> 13
```

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

Elixir and Rust accept only EMRG v9. Re-encode persisted trees and fixtures;
changing only the header is not sufficient for old gradient payloads. Rust's
round-trip serializer preserves raw attr bytes, so byte round-trip tests do not
substitute for decoded-model and render tests.

Standalone `set_attrs` patches have no EMRG header. Their background tags use the
same definitions above: retired background tags `1` and `3` are rejected, so
incompatible patches fail rather than silently changing meaning. Inserted
subtrees contain complete v9 EMRG trees. Upgrade Elixir, the native library,
and the macOS host (handshake v14) together.
