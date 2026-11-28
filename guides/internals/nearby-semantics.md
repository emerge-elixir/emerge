# Nearby semantics

Nearby is authored as an attribute but retained as an ownership edge. [EMRG v10](emrg-format.md) stores an ordinary child list followed by ordered `(slot, id)` mounts. A slot is not an ID: two panels can use the same slot, and a globally keyed panel of the same kind can move between hosts, slots and ordinary children. Removing it from an accepted declaration ends that attachment.

## Slot geometry

The anchor is the host's **local border box**, not its padded content box or its transformed device bounds. Use the panel's own alignment. Mounts don't contribute to the host's intrinsic size, shares, line breaks, floats or ordinary arity.

| Slot | Width offer | Height offer | Origin |
| --- | --- | --- | --- |
| `behind_content`, `in_front` | Host width | Host height | Align on both axes |
| `above`, `below` | Host width | Natural | Align on X; panel bottom at host top, or panel top at host bottom |
| `on_left`, `on_right` | Natural | Host height | Align on Y; panel right at host left, or panel left at host right |

An offer is not a compulsory size. A fixed panel can overflow. For a host at `(10,20)` sized `80×40`, a centered `20×10` panel in `below` starts at `(40,60)`, regardless of host padding.

A paragraph can be a block anchor, but a nonfloating inline continuation is not an anchor. A width-reading mount needs an anchor width available in Flow, not a packed or height-projected width first finalized in Place. A panel whose width computation needs the offered host height requires a proved pixel-height anchor; a shared-flow document paragraph that can outgrow that height doesn't qualify. The proof checks changed edges and ancestor summaries before acceptance. It does not reflow a paragraph twice.

## Paint and hits

`behind_content` stays local, after the host backdrop and before its glyphs, image or ordinary children. The other five slots escape. Menus on sibling hosts share a layer; the later host wins on overlap. A tooltip of either menu is above both menus, and a scrim mounted on their common ancestor is above the tooltip. On one host, later authored escapes win regardless of slot. Plain wrappers do not establish layers.

Escapes retain the host transform and viewport, not ordinary ancestor clips. `clip_nearby/0` explicitly keeps that host's inner clip, including rounded corners, in its own coordinate space. It clips escaped images and outer shadows too. Translucent contributions sharing an opacity ancestor within a layer composite as a group. Zero opacity alone does not remove an input target.

Draw and hit packing follow the same [band order](../../native/emerge_skia/src/tree/paint/bands.rs). A listenerless escape root blocks older targets within its clipped shape; an eligible child listener is above it. Reverse hit testing returns coordinates in the installed listener's local space. A rejected declaration or failed frame handoff doesn't replace installed pixels or hits. This establishes click behavior, not focus order, editor state or scrolling. Scroller-owned versus descendant overlays remain part of the scrolling port.

[Layer decision](retained-decisions.md#d26-stack-nearby-by-layer-then-by-declaration-order) · [native geometry, scopes and admission tests](../../native/emerge_skia/src/tree/tests/nearby.rs) · [headless regression](../../test/emerge_skia/nearby_layers_test.exs).
