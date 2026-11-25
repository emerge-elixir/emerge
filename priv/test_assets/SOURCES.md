Test font fixture source (downloaded 2026-02-24):

- `Lobster-Regular.ttf`: https://raw.githubusercontent.com/google/fonts/main/ofl/lobster/Lobster-Regular.ttf
- `OFL.txt`: https://raw.githubusercontent.com/google/fonts/main/ofl/lobster/OFL.txt

License: SIL Open Font License 1.1

These files are committed so tests can load a custom font fixture without network access.

`single_axis_wide.png` is a generated 200×100 solid-red RGBA fixture for image
aspect-ratio layout tests; it has no external source dependency.
`alpha_grid.png` and `alpha_inverse.png` are generated 10×6 RGBA controls for
blending and same-size replacement. The grid has clear edges, half-alpha red
texels and opaque cyan texels; the inverse control is solid blue.

`turn_right.jpeg` is a generated 16×8 baseline JPEG, displayed as 8×16 by
EXIF orientation 6. Its RGB ramp is `(16*x, 32*y, 15*((3*x+11*y) % 17))`,
encoded with `cjpeg -baseline -quality 95 -sample 1x1` and a little-endian
IFD0 orientation tag. No external image is used.

`media.bmp` and `media.gif` encode the same generated 9×8 swatch: green top
rows, then red and blue columns, with a black marker at x=4 on even rows.
Netpbm `ppmtobmp -windows -bpp 24` produces padded bottom-up BGR;
`ppmtogif -interlace -transparent black` makes the marker transparent.
`sequence.gif` adds a second frame with red and blue interchanged. The static
image decoder draws the first frame.
