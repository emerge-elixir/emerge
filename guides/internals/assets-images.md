# Assets and Images

This guide describes the EMRG v3 image asset pipeline.

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
3. `AssetManager` performs manifest/runtime path resolution and file reads asynchronously.
4. On success, image bytes are decoded and inserted into native cache.
5. `AssetManager` notifies tree actor, which triggers relayout/rerender.

Startup/config flow:

- `EmergeSkia.start/1` calls `configure_assets_nif` with manifest/runtime-path config.
- Rust stores normalized config in `AssetManager` state.
- Reconfiguration clears source-status cache so paths are revalidated under new policy.

Render behavior while waiting:

- pending source -> loading placeholder
- failed source -> failed placeholder
- ready source -> normal image draw

Source status state machine:

- missing -> `pending` (request queued)
- `pending` -> `ready` (decoded + cached)
- `pending` -> `failed` (blocked, unreadable, decode error, or missing)

There is no strict/lenient runtime mode and no fail-fast path for image load
errors. Runtime failures always render the failed placeholder.

## Static Assets (Digest + Manifest)

Run:

```bash
mix emerge.assets.digest
```

This compiles image assets from configured `sources` into a static output root
and emits:

- `cache_manifest.json`
- `cache_manifest_images.json`

`cache_manifest.json` contains:

- `latest` (logical -> digested path)
- `digests` (digested path -> metadata)

`cache_manifest_images.json` is generated for image metadata output and tooling,
while runtime logical path resolution reads `cache_manifest.json`.

## `~m` Verified Media Sigil

`~m"images/logo.png"` returns `%Emerge.Assets.Ref{path: ..., verified?: true}`.

Behavior:

- compile-time validation that the file exists in configured `sources`
- marks source file as external resource for recompilation tracking
- only accepts literal string paths (no modifiers)

Import with:

```elixir
use Emerge.Assets.Path
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

## Config Reference

```elixir
config :emerge_skia, :assets,
  sources: ["assets"],
  manifest: [
    path: "priv/static/cache_manifest.json",
    images_meta_path: "priv/static/cache_manifest_images.json"
  ],
  runtime_paths: [
    enabled: false,
    allowlist: [],
    follow_symlinks: false,
    max_file_size: 25_000_000,
    extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]
  ]
```
