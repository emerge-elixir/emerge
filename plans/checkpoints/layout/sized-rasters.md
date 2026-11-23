# Fitted raster memory

**Complete for retained logical PNG and JPEG.** `assets.decode_at_size: true` retains the fitted bitmap, grows it when either axis is insufficient, and reuses it on smaller draws. Live viewports still default to `false`; one-shot tree exports default to `true`. The before arm was the sealed [JPEG implementation](jpeg.md), not the `3e964e7` commit alone. Pinned reference: `main` `aef5e42fc789c396505f85c34810b2ca9c008586`.

## What stays in memory

For the [640×420 progressive JPEG](../../../priv/sample_assets/static.jpg), intrinsic size never changed. At widths **80 → 160 → 80**, pinned `main` reported the following codec and cache sizes in its live renderer stats. Retained tests checked codec dimensions, the actual Skia pixmap sizes, the resident leases and the number of decodes. Separate test-only observers sampled the bitmap in the installed command on the before and final arms:

| Draw width | Before: bitmap / decoded bytes | Fitted: bitmap / decoded bytes |
| ---: | --- | --- |
| 80 | 640×420 / 1,075,200 | 80×53 / 16,960 |
| 160 | 640×420 / 1,075,200 | 160×105 / 67,200 |
| 80 again | 640×420 / 1,075,200 | **same** 160×105 / 67,200 |

The photo's 37,735 encoded bytes are separate. Replacing a pinned thumbnail charges **both** bitmaps until the old drawing releases its lease. For a 200×100 PNG with a 31×16 target, 80,000 staging bytes and 1,984 result bytes must fit together; only 1,984 decoded bytes survive. Non-identity EXIF orientation also budgets the raw and oriented staging overlap. Neither the table nor the cache settings measure RSS, codec workspace or GPU allocations.

[The loader](../../../native/emerge_skia/src/raster_assets/sized.rs) keeps immutable compressed bytes under a separate 256 MiB per-renderer limit. Original admission limits still apply: 64 MiB per file and per bitmap, at most 32,768 per axis, 256 MiB resident decoded pixels. The cache entry and byte limits may be zero. A frame can pin a bitmap without an LRU entry. [Preparation](../../../native/emerge_skia/src/tree/paint/fits.rs) coalesces visible uses after layout, without doing filesystem or codec work in replay or under the session lock. A hidden image can supply intrinsic dimensions without allocating pixels. See [D18](../../../guides/internals/retained-decisions.md#d18-fit-decoded-memory-to-the-prepared-drawing) and the [loader](../../../native/emerge_skia/src/raster_assets/tests.rs), [tree](../../../native/emerge_skia/src/tree/tests/images.rs) and [viewport regressions](../../../test/emerge_skia/raster_images_test.exs).

## Pixels and input

- Each arm ran 80→160→80 and 160→320→160, with both settings, and fresh one-shot controls at each width. All **24/24** full retained buffers matched pinned `main`. The before arm rendered the full-resolution controls for either setting. A warmed, shrunk thumbnail is not necessarily byte-identical to a fresh thumbnail.
- In the 388-frame matrix (contain/cover, two scales, four box sizes, padding, opacity, PNG, JPEG, EXIF and shared uses), **194/194** opt-out buffers matched the sealed before implementation. Of the 194 sized frames, **153** matched `main`, including **144/144** single-use PNG and non-transposed JPEG controls. Forty visible quarter-turn cases differed around `main`'s failed-image indicator ([D17](../../../guides/internals/retained-decisions.md#d17-bind-jpeg-pixels-in-display-orientation)). All eight display orientations at 3/8 size matched independently `djpeg`-decoded, texel-permuted PNG controls on **both** arms. One shared-source frame differed from cold `main` on the small-first thumbnail (232 pixels); it matched `main` after the larger bitmap had been used. The large-first ordering was exact cold.
- An early 4×3 cover case differed in five edge pixels. Recovering a crop width by subtracting its floating-point edges changed a strict sampling boundary after scaling. Keeping origin and extent separate fixed the complete frames at opacity 1 and 0.5. The [10×9 regression](../../../test/emerge_skia/raster_images_test.exs) checks the rotated equivalent including bleed.
- Opt-out replay of the earlier **228** PNG frames, **229** mounts and **456** patches/undos was unchanged. The prior JPEG independent trips (**87** submissions) and continuous history (**30** mounts, **58** edits/undos) were unchanged too.
- Both arms passed six complete private Sway raster/CPU compositor crops and **24 real button events each**. Every image and following-sibling click reached the viewport; at 80px width the sibling received `(4,3.5)` for surface `(4,56)`, at 160px `(4,3)` for `(4,108)`. The shrink reused the larger pixels but restored the narrow hit geometry. Native tests also hold back or refuse submission and reject obsolete resource completions.

## Qualification

Six release Rust test/strict Clippy profiles (matched, headless, Wayland, GL, Vulkan/deps, Wayland-all): **204/200/203/204/228/228**. Eleven seeded controls at 10,000 cases each. After reservation the interposer measured `[0,0,0]` alloc/realloc/free for discovering a fitted target, binding a decoded result and shrinking with a sufficient bitmap. Decode, raster replay and backend submission are outside those intervals.

Warnings-as-errors compile, format, strict Credo and focused **102/102** passed. Full Mix: **815/882 passed, 67 failed, 21 excluded**, exit **2**. No changed named failures or exclusions versus the sealed JPEG baseline. No latency, hardware GPU or physical scanout claim.

Evidence: `/workspace/emerge-investigation-artifacts/sized-rasters-mucBen/` (`PLAN.md`, `HOWTO.md`, `qualification/`, `logs/main-stats`, `logs/meter-*`, `frames-*`, `matrix-*`, `controls-*`, `prior/`, `window-*`). Final source manifest `7696cbccb5adb193ce03d8ea83134f124306441a97805a0b34c706f37d97764f`; mapped NIF `03cac67e6724e87cf8c11f44112289c3febde4add824aa5e56de72ea402e54b2`.
