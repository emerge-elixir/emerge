# Use assets

This tutorial adds images, SVGs and fonts to a viewport like the one in
[Set up a viewport](set_up_viewport.md). Asset configuration belongs in the
`Emerge.Config` returned by `mount/1`; image sources belong in the UI tree.
You do not need to start a separate renderer.

## Put application files under priv/

Add your files to the application's `priv/` directory, for example:

```text
my_app/
  priv/
    images/logo.png
    images/hero.jpg
    icons/check.svg
```

Use paths relative to `priv/` in UI code: `"images/logo.png"`, not
`"priv/images/logo.png"` or a path relative to the current working directory.
The examples below assume these files exist in your app.

## Display an image in a viewport

Create a gallery viewport:

```elixir
defmodule MyApp.View.Gallery do
  use Emerge

  @impl Viewport
  def mount(_opts) do
    {:ok,
     %Emerge.Config{
       output: %Emerge.Config.Window{title: "Gallery"},
       assets: %Emerge.Config.Assets{otp_app: :my_app}
     }}
  end

  @impl Viewport
  def render() do
    column([padding(16), spacing(12)], [
      image([width(px(120)), height(px(120))], "images/logo.png"),
      svg([width(px(24)), height(px(24))], "icons/check.svg")
    ])
  end
end
```

Start it with `MyApp.View.Gallery.start_link()` or add it to your supervision tree.
`use Emerge` provides the UI helpers; `assets.otp_app` tells Emerge which
application's `priv/` directory to use. Replace `:my_app` with your Mix project's
`:app` name. A viewport can infer this where possible, but declaring it makes the
asset root explicit.

The gallery has no local state, so mount returns `{:ok, config}` and uses
`render/0`. In the counter from the first tutorial, keep
`{:ok, state, config}` and `render/1`; add the assets group to its existing config.

## Check static paths at compile time

String paths are resolved when the renderer loads them. Use `~m` when you want a
missing application file to be caught during compilation instead:

```elixir
defmodule MyApp.UI.Gallery do
  use Emerge.UI
  use Emerge.Assets.Path, otp_app: :my_app

  def layout do
    column([spacing(12)], [
      image([width(px(120)), height(px(120))], ~m"images/logo.png"),
      svg([width(px(24)), height(px(24))], ~m"icons/check.svg")
    ])
  end
end
```

Your viewport can now use `def render(), do: MyApp.UI.Gallery.layout()`.
The sigil checks that the file exists under `priv/` and tracks it as an external
resource. Its `otp_app:` option controls that compile-time check; the viewport's
`assets.otp_app` must still identify the application when running.

## Choose a file or pass bytes

`path/1` declares a filename without opening it. A relative name means the
same thing as a bare string: look under this viewport's application `priv/`.
Use `binary/1` when you already have the encoded file or SVG XML in memory:

```elixir
svg([height(px(24))], path("icons/check.svg"))
svg([height(px(24))], binary(svg_xml))

image([width(px(120))], path("images/logo.png"))
image([width(px(120))], binary(png_bytes))
```

`image/2` accepts PNG, JPEG, still WebP (lossy or lossless, including alpha),
Windows `BI_RGB` BMP (1-, 4-, 8- or 24-bit), GIF frame zero, and original-color
SVG. Animated WebP and compressed BMP are not supported. GIFs do not animate.
`svg/2` accepts only SVG and supports template tinting. Bytes are never treated as a filename, even if
they spell one. Pass encoded data, not base64, raw pixels or a serialized Skia
picture. Empty bytes are invalid. SVG XML is limited to 4 MiB, encoded raster
files to 64 MiB; the tree update including its framing must also fit in 64 MiB.
These byte sources are for image elements, not backgrounds or fonts.

## Size and fit images

Set one dimension to preserve the source's aspect ratio:

```elixir
image([width(px(160))], "images/hero.jpg")
svg([height(px(24))], "icons/check.svg")
```

Set both dimensions when you need a fixed frame, then choose how the source fits:

```elixir
image(
  [width(px(220)), height(px(120)), image_fit(:contain)],
  "images/hero.jpg"
)
```

`:contain` keeps the whole image visible. `:cover` fills the frame and crops the
excess. Use a literal `px/1` for an aspect-preserving fixed size. A bound
is not a literal: for 200×100 art with automatic height, `width(px(40))`
gives 40×20, but `width(min(px(40), px(100)))` gives 40×100 before fitting.

For a responsive image, `width(min(px(80), fill()))` derives the content
height from the allocated width. Conversely, `height(fill())` can derive
width from a definite height, including one inherited through `el` or `row`.
For example, in a 48px-high Column with an 8px sibling and no gap, a 2:1
image gets 40px of height and is 80px wide. A definite-height `wrapped_row`
uses the projected width when choosing its lines. If line membership or a
Column's remaining height already depends on the image width, give the image
a pixel width rather than creating a circular dependency. Aspect ratios use
the space inside borders and padding, in display orientation for JPEG and WebP.

On an image or SVG leaf, `Transform.rotate(45)` turns the painted box and
its click target around the center. `Transform.scale(2)` magnifies both;
`-1` reverses both axes and `0` leaves neither visible art nor a target. These
transforms preserve sibling layout. The image can still overlap a sibling and
receive a click in the overlap.

## Use an image as a background

Use `Background.image/2` on a block `el`, `row`, `column`, `wrapped_row` or
`wrapped_column`. The file paints behind the children, inside the rounded
border box. It doesn't size the box or add an image click target. The default
fit is `:cover`:

```elixir
el(
  [
    width(px(288)),
    height(px(160)),
    padding(12),
    Background.image("images/hero.jpg", fit: :cover),
    Font.color(color(:white))
  ],
  text("Featured trail")
)
```

Use `fit: :contain` to avoid cropping. Use `Background.tiled/1`,
`Background.tiled_x/1` or `Background.tiled_y/1` to repeat a file from the
top-left of the frame. A tile keeps its intrinsic size in device pixels; any
space beyond the tile on a non-repeated axis stays transparent. File
backgrounds are not inline paragraph decorations and don't accept `binary/1`.

Here are styled examples of an image element and a background image:

<img src="assets/assets-image-and-background.png" alt="Image element and image behind foreground content" width="320">

## Tint an SVG

SVGs keep their original colors unless you supply `Svg.color/1`. A solid or
gradient tint colors the composited artwork, preserving transparent holes and
partial coverage. A gradient spans the element's content box:

```elixir
svg(
  [width(px(48)), height(px(48)), Svg.color(color(:sky, 500))],
  "icons/check.svg"
)
```

<img src="assets/ui-assets-svg-example.png" alt="An SVG with original colors and with a tint" width="320">

Use static, self-contained UTF-8 SVG with positive absolute dimensions or a
`viewBox`. Supported artwork includes paths, strokes, local gradients, patterns,
clips, masks, bounded color-matrix filters and text using the bundled Inter and
JetBrains Mono faces. This is not a browser: scripts, animation, stylesheets,
SVGZ, external fonts, and linked or embedded images are not loaded. Unsupported
resources and artwork over the renderer's complexity limits show the failed-image
indicator.

## Register a font in mount

Images are selected in the tree; fonts are registered in `config.assets.fonts`
before the viewport renders. Add your font files under `priv/fonts/`, then return
this configuration from mount:

<!-- emerge-assets:fonts -->
```elixir
%Emerge.Config{
  output: %Emerge.Config.Window{title: "Gallery"},
  assets: %Emerge.Config.Assets{
    otp_app: :my_app,
    fonts: [
      %Emerge.Config.Font{family: "Inter", source: "fonts/Inter-Regular.ttf"},
      %Emerge.Config.Font{family: "Inter", source: "fonts/Inter-Bold.ttf", weight: 700},
      %Emerge.Config.Font{family: "Inter", source: "fonts/Inter-Italic.ttf", italic: true}
    ]
  }
}
```

Return the record as `{:ok, config}` for the gallery or `{:ok, state, config}` for
a stateful viewport. These paths require the regular, bold and italic files to
exist in your application's `priv/fonts/` directory.

Select the registered family and variants in render:

<!-- emerge-example:ui-assets-font-variants -->
```elixir
column(
  [
    width(fill()),
    height(fill()),
    padding(24),
    spacing(8),
    Background.color(color(:slate, 900)),
    Font.family("Inter")
  ],
  [
    el([Font.size(22), Font.bold(), Font.color(color(:slate, 50))], text("Release notes")),
    el([Font.regular(), Font.color(color(:slate, 200))], text("Design system updated")),
    el([Font.italic(), Font.color(color(:slate, 300))], text("Beta"))
  ]
)
```

<img src="assets/ui-assets-font-variants.png" alt="Regular, bold and italic font variants" width="320">

`Font.family/1` selects the family; `Font.bold/0` or `Font.weight(700)` selects
the registered bold variant; `Font.italic/0` selects the italic variant.
Register the variants you use rather than expecting a regular file to provide
all weights and styles. Font registrations require a new viewport to change.

## Load user-selected files

Files such as downloaded photos need runtime paths rather than `~m`. Enable only
the directories your application needs, by adding a `RuntimePaths` record to its
assets configuration:

<!-- emerge-assets:runtime -->
```elixir
%Emerge.Config{
  assets: %Emerge.Config.Assets{
    otp_app: :my_app,
    runtime_paths: %Emerge.Config.RuntimePaths{
      enabled: true,
      allowlist: ["/data/photos"]
    }
  }
}
```

Then pass an absolute path in the UI:

```elixir
image([width(px(160)), height(px(96))], path("/data/photos/photo.jpg"))
```

Runtime loading is off by default. An enabled policy with an empty allowlist
still allows no files. Symlinks are rejected by default; if you allow them,
resolved paths must remain in an allowed directory. Keep existing font
registrations in the same assets record when adding this setting.

These settings are chosen at startup. Once the policy is configured, you can
change the image path in your UI without changing config or restarting the viewport.
See `Emerge.Config.RuntimePaths` for file-size and extension restrictions.

## Optional: reduce image memory

Most apps can keep the cache defaults. For a grid of thumbnails from large photos,
consider decoding images near their drawn size and lowering retained-pixel budgets:

<!-- emerge-assets:cache -->
```elixir
%Emerge.Config{
  assets: %Emerge.Config.Assets{
    otp_app: :my_app,
    decode_at_size: true,
    cache: %Emerge.Config.AssetCache{max_entries: 32, max_bytes: 32 * 1024 * 1024}
  }
}
```

With `decode_at_size: true`, enlarging a thumbnail can grow its decoded bitmap.
Shrinking it again reuses the larger bitmap while available, without changing
its intrinsic layout size. Some codecs, notably PNG, still need a larger
temporary decode even when only the thumbnail is retained.

`max_entries` and `max_bytes` limit cached raster pixels, not compressed file
sizes. SVGs instead share one recorded picture for identical content within a
renderer, regardless of size, fit or tint. `decode_at_size` has no effect on SVG.
`svg_tree_max_entries` and `svg_tree_max_bytes` let unused pictures stay cached
(default 64 pictures / 16 MiB estimated storage). Set either to zero to disable
idle picture retention; live uses still share. These are per-renderer budgets,
not limits on total application or peak decoding memory. Keep any fonts/runtime-path policy alongside these fields when combining
settings. See `Emerge.Config.Assets` and `Emerge.Config.AssetCache` for defaults.

To see periodic performance and asset-memory summaries, add a diagnostics group
to your existing config:

<!-- emerge-assets:diagnostics -->
```elixir
%Emerge.Config{
  diagnostics: %Emerge.Config.Diagnostics{logs: [:stats]}
}
```

This prints summaries rather than requiring you to poll a renderer. For programmatic
measurements instead, see `Emerge.Config.Diagnostics`.

## While an image loads

Emerge loads images without blocking rendering. Give images a fixed frame when
layout should not move as a file loads. Slow loads show a small indicator;
failed loads show an error placeholder. A content-sized image needs its source
dimensions before its final size is known.

## Next

Continue with [Manage state](state_management.md) to move application state out
of the viewport as the UI grows.
