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

`stills/swatch.png`, `stills/swatch.bmp` and `stills/indexed{1,4,8}.bmp` are
lossless encodings of a generated 16×8 red-left/blue-right PPM. The BMPs use
Netpbm `ppmtobmp -windows -bpp N` (N=24, 1, 4, 8). The native tests also build
3-pixel top-down rows with deliberately unused nonzero padding and short tables.

The WebP fixtures in `stills/` are generated art. FFmpeg n9.0.2 with libwebp
1.6.0 encoded the swatch as VP8L (`lossless`) and VP8 (`lossy`, `yuv420p`,
quality 85). `alpha` is `alpha_grid.png` encoded at quality 85 as `yuva420p`
(VP8X+ALPH); `clear` is its lossless-alpha VP8L counterpart. The matching
RGBA PNGs were decoded with FFmpeg's native `webp` decoder, not Skia. `turn`
wraps `clear` in VP8X with a little-endian TIFF orientation 6; its PNG is the
independently decoded PNG transposed clockwise. `animated.webp` is a two-frame
lossless encoding of `alpha_grid.png` then `alpha_inverse.png` (negative).
[SHA-256 checksums](stills/SHA256SUMS) pin these small inputs and references.

The lossy pixel checks use decoded reference colors rather than the source
PPM. Before testing the renderer, `WebPDecodeRGBA` vs FFmpeg differed by at
most 1 per channel inside the flat swatch and 19 at the selected grid points
(x=2/7, y=2/3, straight RGBA). Allow 1 and 20 respectively in submitted
premultiplied RGB (including rounding); alpha must agree everywhere. Lossless
stills are compared exactly over black and white. No external decoder or
encoder is needed to run the tests.

### Reproduce the still fixtures

In an empty directory, set `src` to the absolute path of `priv/test_assets`.
These commands reproduce `stills/SHA256SUMS` with the versions above and
Netpbm 10.86.51. The reference PNGs are generated independently of the renderer.

```sh
elixir -e 'row = [:binary.copy(<<255,0,0>>,8), :binary.copy(<<0,0,255>>,8)]; File.write!("swatch.ppm", ["P6\n16 8\n255\n", List.duplicate(row,8)])'
pnmtopng swatch.ppm >swatch.png
ppmtobmp -windows -bpp 24 swatch.ppm >swatch.bmp
for n in 1 4 8; do ppmtobmp -windows -bpp "$n" swatch.ppm >"indexed$n.bmp"; done
ff() { ffmpeg -nostdin -v error -threads 1 "$@"; }
ff -i swatch.ppm -c:v libwebp -lossless 1 lossless.webp
ff -i swatch.ppm -c:v libwebp -pix_fmt yuv420p -q:v 85 lossy.webp
ff -i "$src/alpha_grid.png" -c:v libwebp -pix_fmt yuva420p -q:v 85 alpha.webp
ff -i "$src/alpha_grid.png" -c:v libwebp -lossless 1 clear.webp
for f in lossless lossy alpha clear; do
  ff -c:v webp -i "$f.webp" -frames:v 1 -pix_fmt rgba -threads 1 "$f.png"
done
elixir -e '
  <<"RIFF", _::32, "WEBP", image::binary>> = File.read!("clear.webp")
  tiff = <<"II", 42::little-16, 8::little-32, 1::little-16, 274::little-16,
           3::little-16, 1::little-32, 6::little-16, 0::16, 0::32>>
  body = <<"WEBPVP8X", 10::little-32, 24, 0::24, 9::little-24, 5::little-24,
           image::binary, "EXIF", byte_size(tiff)::little-32, tiff::binary>>
  File.write!("turn.webp", <<"RIFF", byte_size(body)::little-32, body::binary>>)'
ff -i clear.png -vf transpose=clock -pix_fmt rgba -threads 1 turn.png
cp "$src/alpha_grid.png" 0.png; cp "$src/alpha_inverse.png" 1.png
ff -framerate 1 -i %d.png -frames:v 2 -c:v libwebp_anim -lossless 1 -loop 0 animated.webp
sha256sum -c "$src/stills/SHA256SUMS"
```
