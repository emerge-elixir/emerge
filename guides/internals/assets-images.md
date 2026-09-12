# Assets and Images

This guide describes the EMRG v3 image asset pipeline.

`image/2` and `Background.image/2` support raster formats plus self-contained SVGs.
SVG text uses system font matching; relative subresources and external SVG fonts are not loaded in v1.

## Design Goals

- Keep UI APIs source-based (`~m"..."`, logical paths, runtime paths).
- Keep source I/O off the render-critical path.
- Decode rasters at their fitted device-space size when requested.
- Bound retained decoded pixels independently from encoded source records.
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
4. Raster sources retain encoded metadata until drawing knows the fitted target;
   SVGs are parsed into vector trees.
5. During draw, raster lookup computes the fitted device-space dimensions. With
   sized decode enabled, the codec produces the smallest non-undersized staging
   image it supports and the renderer resamples once to the exact target.
6. The final raster enters the entry/byte-bounded LRU. SVG drawing uses a
   separate bounded rendered-variant cache.
7. `AssetManager` notifies tree actor, which triggers relayout/rerender.

Startup/config flow:

- `EmergeSkia.start/1` requires `otp_app` and calls `configure_assets_nif` with `<otp_app>/priv` as the source root, runtime-path policy, raster-cache limits, and sized-decode policy.
- `EmergeSkia.start/1` preloads configured font assets (`assets.fonts`) from `<otp_app>/priv` and registers them in the native font cache.
- Rust stores normalized config in the renderer's asset runtime and applies raster-cache limits to that renderer's decoded LRU.
- Reconfiguration clears that renderer's source state and SVG caches so paths and SVG parsing are revalidated under the new policy. Other renderers are unaffected.

Render behavior while waiting:

- pending source -> loading placeholder
- failed source -> failed placeholder
- ready source -> normal image draw

Source status state machine:

- missing -> `pending` (request queued)
- `pending` -> `ready` (encoded raster metadata or a parsed vector is available)
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

## Shared decoded-pixel retention

`assets.cache.max_entries` and `assets.cache.max_bytes` default to 256 entries
and 256 MiB. Raster images and rasterized SVG variants share **one LRU and one
budget**. Each SVG size/fit variant counts as an entry; all retained Skia pixel
storage contributes to the byte total. Encoded raster source bytes, parsed SVG
trees, and SVG font discovery are tracked separately.

One content ID retains at most one raster. A retained raster is reused when both
its dimensions are at least the requested target. A larger target replaces it
with a larger decode. A smaller target does not create another variant.

A zero entry limit, zero byte limit, or image larger than the byte limit skips
retention without skipping the draw. Eviction is least-recently-used within the
renderer's cache.

Encoded source status and decoded retention have independent lifetimes. A
retained raster can render while an evicted source record is hydrated again.
Generation checks prevent reuse after source content changes.

### SVG font, tree, and pixel caches

SVG loading has three independent renderer-local cache stages:

- **Font discovery:** one immutable system-font database per asset configuration
  generation, shared across SVG documents and parsed trees. This preserves the
  existing system-font matching behavior; it is not the Skia registered-font cache.
- **Parsed trees:** an `Arc<usvg::Tree>` is cached immediately after parsing, before
  any rasterization. `assets.cache.svg_tree_max_entries` (default 64) and
  `assets.cache.svg_tree_max_bytes` (default 16 MiB) bound this LRU separately.
  Bytes are an estimated tree-owned heap charge, including structure, path/text
  buffers, definitions, and embedded image bytes, with shared subobjects deduplicated.
  Private usvg fields receive an allowance; this is not an exact RSS measurement.
  The shared font database is estimated separately once, not charged once per tree.
  That estimate covers database metadata and deduplicated retained binary/mapped
  buffers, not unloaded font-file contents or an exact resident-memory total.
- **Rasterized pixels:** each exact size/fit variant is retained in the shared
  decoded-pixel LRU above. There is no separate SVG pixel budget or 1 MiB
  per-variant cap. Tint is applied during drawing and does not duplicate variants.

Removing an SVG from the scene releases active references, not retained trees or
pixels. Reopening at the same cached size requires no discovery, parsing, or
rasterization. A new size is rasterized directly from a retained tree for full
quality; it does not enlarge a smaller bitmap or show a loading placeholder.
A complex new rasterization can still take time on the render thread.

Tree and pixel eviction are independent. A pixel hit works without a retained
tree; a tree hit works without retained pixels. Only a miss in both requires
asynchronous loading and a placeholder. Zero tree-cache limits disable tree
retention without disabling font-database reuse. Oversized trees remain usable
by active draws but are not retained after use.

Remounted path sources are validated against the current source policy and
revalidated asynchronously. Authorized cached content can be displayed while
revalidation runs. Unchanged content preserves trees and pixels; changed bytes
publish a new content ID. Deletion/read failure publishes the failed placeholder.
Reconfiguration invalidates SVG trees, pixels, source mappings, and the SVG font
environment generation. Renderer recreation is a cold start.

Cache limits bound cache-owned storage, not active/in-flight references or all
process memory. Parsed-tree estimates and font data are not included in the
shared pixel budget. SVG parsing and CPU rasterization remain available on both
embedded and desktop builds.

Each native renderer owns its source worker/configuration, encoded source
records, decoded raster LRU, rendered SVG variants, registered fonts, and cache
generations. Renderer shutdown joins only that renderer's worker and clears only
that renderer's asset state.

## Memory Diagnostics

`renderer_stats_log` includes:

- retained encoded source count and bytes;
- decoded raster entries, bytes, and configured limits;
- shared pixel-cache entries, bytes, and configured limits, with raster/SVG breakdowns;
- SVG variant source IDs, fit variant, rasterized dimensions, and retained bytes;
- parsed SVG cache entries, estimated bytes/limits, hits, misses, and evictions;
- SVG font-environment generation, face count, separate estimated storage, and discovery/parse/rasterization counts;
- original, codec, and final dimensions per retained raster;
- decoded-to-source pixel ratio, decoded-to-file byte ratio, estimated peak
  decode bytes, and whether the encoded source record is retained.

The diagnostics are routed through `NativeLogRelay`, not emitted directly from
the decode worker.

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
    decode_at_size: true,
    cache: [
      max_entries: 32,
      max_bytes: 32 * 1024 * 1024,
      svg_tree_max_entries: 32,
      svg_tree_max_bytes: 8 * 1024 * 1024
    ],
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
