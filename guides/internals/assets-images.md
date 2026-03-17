# Assets and Images

This guide describes the EMRG v3 image asset pipeline.

`image/2` and `Background.image/2` support raster formats plus self-contained SVGs.
SVG text uses system font matching; relative subresources and external SVG fonts are not loaded in v1.

## Design Goals

- Keep UI APIs source-based (`~m"..."`, logical paths, runtime paths).
- Keep runtime loading and decoding off the render-critical path.
- Resolve and cache assets in Rust asynchronously.
- Never fail fast on missing runtime media: show loading/failed placeholders.

## Source Types

`image/2` and `Background.image/2` support:

- `%Emerge.Assets.Ref{}` from `~m"..."`
- logical path string (example: `"images/logo.png"`)
- runtime path tuple (example: `{:path, "/data/photos/a.jpg"}`)
- preloaded image ID tuple (example: `{:id, "img_<sha256>"}`)

In EMRG v3 these are encoded as typed image sources:

- `0` -> `{:id, id}`
- `1` -> logical path
- `2` -> `{:path, path}`

## Runtime Flow

1. Elixir uploads/patches tree sources as-is (no Elixir-side file IO).
2. Rust tree actor requests missing sources from `AssetManager` actor.
3. `AssetManager` resolves logical paths from the configured OTP app `priv` root (or validates runtime paths) and reads files asynchronously.
4. On success, raster bytes are decoded or SVGs are parsed into a cached vector tree.
5. `AssetManager` notifies tree actor, which triggers relayout/rerender.

Startup/config flow:

- `EmergeSkia.start/1` requires `otp_app` and calls `configure_assets_nif` with `<otp_app>/priv` as the source root plus runtime-path policy.
- `EmergeSkia.start/1` preloads configured font assets (`assets.fonts`) from `<otp_app>/priv` and registers them in the native font cache.
- Rust stores normalized config in `AssetManager` state.
- Reconfiguration clears source-status cache so paths are revalidated under new policy.

Render behavior while waiting:

- pending source -> loading placeholder
- failed source -> failed placeholder
- ready source -> normal image draw

Source status state machine:

- missing -> `pending` (request queued)
- `pending` -> `ready` (decoded/parsed + cached)
- `pending` -> `failed` (blocked, unreadable, decode error, or missing)

There is no strict/lenient runtime mode and no fail-fast path for image load
errors. Runtime failures always render the failed placeholder.

## Source Root

Logical sources are resolved directly from the `priv` root of the `otp_app` passed to `EmergeSkia.start/1`.

Path safety rules for logical sources:

- paths must be relative (leading `/` is normalized away)
- `..` traversal is rejected
- missing files resolve to the failed placeholder path

## `~m` Verified Media Sigil

`~m"images/logo.png"` returns `%Emerge.Assets.Ref{path: ..., verified?: true}`.

Behavior:

- compile-time validation that the file exists under `<otp_app>/priv`
- marks source file as external resource for recompilation tracking
- only accepts literal string paths (no modifiers)

Import with:

```elixir
use Emerge.Assets.Path, otp_app: :my_app
```

## Runtime Paths (Security)

Runtime filesystem ingestion is controlled by `runtime_paths` config.

Defaults are restrictive:

- `enabled: false`
- empty allowlist
- symlink following disabled
- extension allowlist enforced
- max file size enforced

Validation sequence for runtime paths:

1. file stat
2. extension check
3. file size check
4. symlink/canonical path policy
5. allowlist root check

## Font Assets

Font assets are configured at startup under `assets.fonts` and loaded synchronously.

Each entry supports:

- `family` (required)
- `source` (required logical path under `<otp_app>/priv`, or `%Emerge.Assets.Ref{}`)
- `weight` (optional, default `400`)
- `italic` (optional, default `false`)

Duplicate variants (`{family, weight, italic}`) are rejected at startup.

## Start Options

```elixir
EmergeSkia.start(
  otp_app: :my_app,
  assets: [
    fonts: [
      [family: "my-font", source: "fonts/MyFont-Regular.ttf", weight: 400],
      [family: "my-font", source: "fonts/MyFont-Bold.ttf", weight: 700],
      [family: "my-font", source: "fonts/MyFont-Italic.ttf", weight: 400, italic: true]
    ],
    runtime_paths: [
      enabled: false,
      allowlist: [],
      follow_symlinks: false,
      max_file_size: 25_000_000,
      extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]
    ]
  ]
)
```
