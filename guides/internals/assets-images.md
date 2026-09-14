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

## Retained scene image bindings

Native frame preparation captures source statuses, dimensions, status generation
and immutable image bindings together. Referenced data is cloned under the existing
source-state → pixel-cache lock order with matching configuration epochs, then both
locks are released. Preparation, native layout and scene assembly use that same
input even if a loader finishes, an ID is replaced or configuration resets between
them. The tree keeps it across split layout/refresh calls; failed preparation does
not replace the previously published frame input. Historical endpoint queries keep
dimensions, not full asset snapshots.

The declared-source index includes inactive interaction backgrounds and is cached
by model/revision, with unknown mutable access invalidating it. Background-only
changes do not invalidate intrinsic measurement; changed image dimensions dirty
image nodes and their native dependency paths. Frames with no image references
skip asset-cache locking. Lookups during a frozen frame do not register sources or
queue new loads; a new preparation registers live references before capture.

Native tree publication retains the image IDs referenced by its render graph.
Each scene shares immutable encoded/parsed source records, or pins existing cached
pixels when the source record is unavailable. Re-registering an ID, evicting a
cache entry or resetting the renderer's live asset state does not replace the
image inside an already-captured scene. Absent bindings do not fall through to a
new registration. A newly published scene captures the new image.

Capture performs no file I/O, hydration, decoding or rasterization. Painting uses
short lookup locks only; no asset lock is held across decoding or Skia drawing.
Cached pixels include source identity as well as renderer-local generation. Old
or foreign scene records can be drawn without publishing their metadata back
into the current renderer's asset cache.

Scene references can outlive cache eviction. `ImageSnapshot::retention()` reports
per-snapshot record counts, encoded-byte charges and pinned cached-pixel charges;
these are not globally deduplicated live heap/GPU bytes, and parsed SVG memory is
not included in the encoded-byte count. `capture_node_visits()` reports actual
render-graph traversal work. Prepared scenes with no declared image references
skip that traversal; standalone capture also skips it when no source records or
cached pixels exist. Otherwise capture walks the render graph. Per-scene charges
do not include the tree's declared-source index, frame-status maps or image-node
measurement index. These costs have not been benchmark-qualified.

This contract covers prepared native frames. Broader combined-input schedules and
platform presentation remain under qualification; it is not a global transaction
across independent font and asset registries. Manually constructed low-level Rust
scenes with `images: None` preserve their existing live-ID rendering behavior.

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
