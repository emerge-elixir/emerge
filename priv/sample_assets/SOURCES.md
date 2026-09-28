# Sample assets

These assets ship in the Hex package so documentation examples and renderer
benchmarks work offline.

## Sample photos — Unsplash License

Both JPEGs were obtained through Lorem Picsum. On 2026-09-28, fresh downloads
from the URLs below matched the vendored files byte-for-byte. Picsum's image
metadata identifies the original photographer and Unsplash page.

| File | Photographer | Original | Download / metadata |
|---|---|---|---|
| `static.jpg` | Andrew Ridley | https://unsplash.com/photos/Kt5hRENuotI | https://picsum.photos/id/1018/640/420 / https://picsum.photos/id/1018/info |
| `fallback.jpg` | Christian Joudrey | https://unsplash.com/photos/mWRR1xj95hg | https://picsum.photos/id/1043/640/420 / https://picsum.photos/id/1043/info |

SHA-256:

```text
d51425315e432c0354991640fa5ba29b8022024850ab61087fc6624cae35beb0  static.jpg
bba736a9e8c9e06490cdc00f03e54d5754a36c148c519681db937ab88587877d  fallback.jpg
```

The [Unsplash License](https://unsplash.com/license) permits copying and
redistribution, including commercial use. It prohibits selling images without
significant modification and compiling them into a competing image service.
These files are sample/example assets, not an image service or standalone photo
product. The license text and restrictions are included in
`licenses/Unsplash.txt` at the package root. Attribution is not required by that
license; these credits identify the samples and photos in generated screenshots.

## SVGs

- `tile_quad.svg`: generated in-repo as a 2x2 test pattern, under Emerge's Apache-2.0 license.
- `template_cloud.svg`: adapted from
  https://github.com/tabler/tabler-icons/blob/master/icons/outline/cloud.svg.
  The geometry is unchanged, with sample-specific stroke colors. Tabler icons
  are MIT licensed; the license is included in `licenses/Tabler-icons-MIT.txt`
  at the package root.
