# Viewport config schema — implemented 0.5 API

Parent: [declarative viewport config](active-declarative-viewport-settings.md).
The typed structs, normalization, previous-config callbacks and raw input API are
implemented. This file is an implementation index, not a second proposed schema.
Authoritative user documentation:

- [Configuration reference](../guides/reference/viewport-configuration.md)
- [0.4-to-0.5 migration and old option mapping](../guides/migrations/0.5.md)
- [Raw input contract](../guides/tutorials/handle_input.md)
- Module docs/types in `lib/emerge/config*.ex` and `lib/emerge/input.ex`

The package/artifact manifests target 0.5.0. Publication still requires the exact
release commit's CI and rebuilt artifacts; see the native build guide.
No compatibility conversion layer remains for replaced mount/start config, aliases,
public numeric masks or adapter startup contracts. Build configuration and application
mount/process arguments are separate APIs and are not removed by this change.

## Canonical groups and variants

| Root field | Records |
|---|---|
| `output` | Exactly one `Window`, `DRM` or `Headless` |
| `renderer` | `Renderer`, `RendererCache`, `PaintLayerCache` |
| `input` | `Input`, `Cursor`, optional `CursorIcon` overrides |
| `assets` | `Assets`, `Font`, `RuntimePaths`, `AssetCache` |
| `diagnostics` | `Diagnostics` |
| `runtime` | `Runtime` |

All names are under `Emerge.Config`. Fixed-shape fields accept their struct or
explicitly typed keyword shorthand. `output` and `Headless.frames` variants are
explicit structs; the latter is `BinaryFrames` or `PrimeFrames`. Records/lists
replace, not deep-merge or append. Normalization expands defaults, rejects unknown/
duplicate fields and returns nested structs. Viewport assets infer the app where
possible; standalone startup requires it, and headless requires a live local sink.

```elixir
%Emerge.Config{
  output: %Emerge.Config.DRM{connector: "DSI-1", backlight: [brightness: 0.7]},
  renderer: [api: :opengl],
  input: [events: [:key, :resize]]
}
```

## Callback and intent

Mount returns the root config, with optional map state. Implement at most one of
`config(previous)` and `config(previous, state)`. First evaluation precedes renderer
creation; later evaluations are coalesced before rendering, including while off.
Previous is normalized desired intent, not hardware/applied state. Typed struct
updates preserve other fields. Invalid results preserve previous intent; operational
errors preserve latest desired intent for full-snapshot retry and recovery.

Raw observation defaults to `[]`; `Input.all_events/0` returns the nine category
atoms, including contact-aware `:touch`. Pointer/resize/focus callback names are coherent with those categories and
press/release actions are atoms. Numeric masks stay private. Subscriptions do not
disable element interactions or window close. Backend availability and exact text/
key/IME/coordinate contracts are in the input guide, not invented by the schema.

## Current live/static boundary

- Live: `input.events`, runtime health interval, Wayland/macOS titles,
  headless/Wayland render-admission caps and explicit OpenGL/CPU-raster DRM power/
  brightness. `nil` caps restore backend pacing, not continuous redrawing.
- Rejected live: connector/mode changes and DRM/macOS caps.
- Startup-only: output type/card/size, renderer API/device/cache, cursor, assets,
  diagnostics, backlight device and adapter identity/options.
- Vulkan and raster GPU-upload power remain rejected. General Linux GPU/PRIME/
  reconnect qualification is closed; the specific Wayland Vulkan acquire hazard
  remains deferred in [its own plan](active-vulkan-acquire-synchronization.md).
- macOS titles/input are acknowledged through the host; native config revision
  reporting and structured renderer stats API remain unsupported there.

## Power scope

Off retains output/backlight/tree ownership and intent, ignores UI input, and
stops/destroys the presenter. On creates fresh resources and requests the latest
tree, then illuminates a managed backlight after first application presentation.
Initially-off acquires/routes the owner without upload. No GPU restore, config-tree
markers, frame receipts, power input epochs or custom commit cookies.

Auto backlight association must be proven and unambiguous; nil opts out and a path
binds explicitly. Brightness is finite `0..1`, zero is not display-off, and nil
restores the original requested baseline retained across renderer recovery.
Unsafe cleanup retains ownership. Physical no-flash and panel-driver behavior
remain device qualification, not promises of schema validation. See
[native power](active-native-drm-power.md) for the current handoff.
