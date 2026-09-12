# Universal gradient colors

`Emerge.UI.Color.gradient/1,2` returns `{:color_gradient, colors, angle}`. It is
accepted by backgrounds, fonts, borders, shadow colors and SVG template tint,
including their existing state-style and animation slots. See the
[migration guide](../migrations/0.4.md) and [wire format](emrg-format.md).

## Validation and native model

The constructor, attribute validation and direct encoding validate proper lists
of 2–u32-max solid stops, byte channels and finite f64-compatible angles. Gradient
stops cannot themselves be gradients. SVG-specific extra attributes pass through
value validation; the separately validated slider configuration is unaffected.

Native `attrs::Color` includes a gradient whose shared stop array contains only
`SolidColor`. Backgrounds use `Background::Color` for both solids and gradients;
there is no separate gradient background model. EMRG v9 encodes a gradient with
color tag `3`, including inside background variant `0`. Background variants `1`
and `3` are retired. The macOS handshake is v13.

A shared `RenderColor` stores resolved RGBA or a linear brush with shared RGBA
stops, normalized angle and reference box. Font inheritance and paragraph
fragments preserve this descriptor rather than reducing it to a scalar color.
They bind the reference box at paint preparation; glyphs/fragments share stop
storage. Identical resolved stops may take the solid render fast path without
changing the serialized/public value.

## Geometry

All brushes use the existing centered diagonal-length axis, evenly spaced stops,
Clamp tiling and Skia interpolation. Angles are reduced modulo 360 in f64 before
f32 conversion; axis arithmetic uses f64 and rejects invalid shader coordinates.

- Backgrounds and borders use the outer border box, shared across edges/dashes.
- Text uses the text owner's content box. Paragraph fragments (including inline
  overrides) share the paragraph content box; multiline text shares one content
  box across lines. Glyphs, words and decorations do not restart the gradient.
- Shadows use the casting border box before offset, spread and blur. Existing
  blur coverage, transparent centers and inset clipping are unchanged.
- SVG tint uses the element's content box. Fit changes coverage, not the gradient
  reference. Repeats tile the source mask, not the gradient. Public `image_fit/1`
  retains its contain/cover contract; native repeat paths also support brushes.

Transforms and retained-payload translations move the brush and geometry together.
Clipping/cropping must not rebase the reference box. Resize changes it deliberately.

## Rendering and caches

The shared paint builder supplies color or shader to ordinary rectangle, text,
border and shadow primitives. Gradient backgrounds are brush-bearing rectangles,
not a special primitive. These ordinary paint paths do not require extra layers.

SVG solid tints retain their color-filter fast path. Gradient tint uses an isolated,
clipped template layer and `SrcIn`: output RGB comes from the gradient, with alpha
multiplied by source alpha exactly once. Holes, antialiased edges and partial
opacity survive. Cover rendering keeps its viewport-sized raster variant; tint
never requires rasterizing a full cover-scaled source. Loading/error placeholders
are not tinted. Parsed trees and raster variants stay tint-independent.

Brush hashes include stops, angle and reference geometry. Paint invalidation and
retained paragraph refresh update inherited colors without putting color into
measurement keys. Grayscale policy remains role-specific: gradient text/borders
and SVGs retain their protection role, and varying background gradients remain
ditherable. Policy recoloring preserves all stop alpha and SVG source alpha.

## Animation

Solid endpoints interpolate as before. Equal-count gradients interpolate each
stop and angle. Solid/gradient pairs lift the solid to the gradient's count and
angle. Authored gradient counts must agree throughout the keyframe sequence,
even when solid keyframes intervene; shadow entries are checked individually.
Incompatible runtime-generated gradient counts hold the source during sampling
and reach the target at completion rather than truncating stops.

## Verification

- `test/emerge/gradient_test.exs`: constructors, real SVG validation, shared wire
  encoding, state styles, compatibility and malformed inputs.
- `test/emerge_skia/gradient_test.exs`: every public consumer's pixels, solid
  equivalence, native round trips, retained inherited paragraph/SVG patches,
  transparent SVG regression, Gray8 and packed grayscale output.
- `renderer_gradient_tests.rs` and native tree/scene tests: SVG source-alpha
  multiplication for all native fits, untinted cache reuse, bounded tint layers,
  grayscale roles, animation, malformed payloads, paint hashes and cache reuse.
- Renderer Criterion cases `gradient_text`, `gradient_borders`,
  `gradient_shadows`, `gradient_svg_tint` and `gradient_rects` exercise raster/GPU
  paths. Run measurements under the exclusive lock described in
  `bench/README.md`.
