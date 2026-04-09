# Third-Party Assets

This repository includes a small number of bundled non-code third-party assets.
This file lists package/runtime-relevant assets first, then repo-only assets
that are retained for docs and tests in this repository.

For a concise downstream redistribution summary of packaged/runtime-relevant
assets, see `NOTICE`.

Scope markers used below:

- Package/runtime-relevant: shipped in the Hex package and/or bundled into
  runtime artifacts.
- Repo-only: retained in the git repository but not shipped in the Hex package.

## Package/runtime-relevant assets

### Inter default fonts

- Paths:
  - `native/emerge_skia/src/fonts/Inter-Regular.ttf`
  - `native/emerge_skia/src/fonts/Inter-Bold.ttf`
  - `native/emerge_skia/src/fonts/Inter-Italic.ttf`
  - `native/emerge_skia/src/fonts/Inter-BoldItalic.ttf`
- Purpose: embedded as the renderer's default bundled fonts via `include_bytes!`.
- Upstream: `https://github.com/rsms/inter`
- License: SIL Open Font License 1.1
- License text: `native/emerge_skia/src/fonts/OFL.txt`
- Source note: `native/emerge_skia/src/fonts/SOURCES.md`

### Mocu DRM cursor SVGs

- Path: `native/emerge_skia/src/backend/drm/cursors/mocu_black_right/`
- Purpose: default DRM cursor assets.
- Upstream: `https://github.com/sevmeyer/mocu-xcursor`
- License: CC0 1.0 Universal
- License text:
  `native/emerge_skia/src/backend/drm/cursors/mocu_black_right/LICENSE-CC0.txt`
- Source note:
  `native/emerge_skia/src/backend/drm/cursors/mocu_black_right/SOURCES.md`

These package/runtime-relevant notices are also summarized in `NOTICE`.

## Repo-only docs/test assets

### Lobster test font fixture

- Paths:
  - `priv/test_assets/Lobster-Regular.ttf`
- Purpose: test fixture for custom font loading.
- Upstream: `https://github.com/google/fonts/tree/main/ofl/lobster`
- License: SIL Open Font License 1.1
- License text: `priv/test_assets/OFL.txt`
- Source note: `priv/test_assets/SOURCES.md`

### Tabler-derived sample SVG

- Paths:
  - `priv/sample_assets/template_cloud.svg`
- Purpose: offline docs examples and renderer tests.
- Upstream:
  - `https://github.com/tabler/tabler-icons/blob/master/icons/outline/cloud.svg`
- License: MIT
- License text: `licenses/Tabler-icons-MIT.txt`
- Source note: `priv/sample_assets/SOURCES.md`

### Placeholder sample photos

- Paths:
  - `priv/sample_assets/static.jpg`
  - `priv/sample_assets/fallback.jpg`
- Purpose: offline docs/examples assets only.
- Source note: `priv/sample_assets/SOURCES.md`
- Note: these placeholder photos are retained for offline repo use and are not
  included in the Hex package. Re-verify provenance and redistribution terms
  before reusing them outside this repository.

## Generated in-repo assets

- Documentation screenshots under `assets/` and `guides/tutorials/assets/` are
  generated from this repository's own source code and are not third-party asset
  imports.
