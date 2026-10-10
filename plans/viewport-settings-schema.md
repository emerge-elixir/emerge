# Viewport config: current schema and proposed display/backlight additions

Implementation in progress. Parent: [declarative viewport config and progress](active-declarative-viewport-settings.md). The callback is `config`, not `settings`, and receives the previous config. Implemented arities: `config(previous_config)` or `config(previous_config, state)`.

The structs, shorthand normalization, callbacks and typed input API now exist in `lib/emerge/config*` and `lib/emerge/input.ex`. This document retains the full target schema; **native DRM display/backlight control, live frame caps/output reconfiguration and macOS titles are still pending**. CPU DRM first-upload readiness and Wayland title updates are implemented. See the parent progress checklist before treating any field as supported hardware behavior.

Target: **0.5.0**, intentionally breaking. Old options below are a migration reference, not a compatibility contract. No keyword-config conversion layer, deprecated aliases or public numeric-mask compatibility will be carried forward.

## Current structure (0.4 reference)

This is the mount/start option inventory, not an existing live `config` API. Sources: `lib/emerge/runtime/viewport/config.ex`, `lib/emerge_skia/options.ex`, `lib/emerge_skia/assets.ex`.

Renderer options may be top-level or under `emerge_skia: [...]`; nested values override duplicate top-level values. `viewport: [...]` contains runtime/adapter options.

The following shows defaults except for the illustrative `otp_app`. Backend-specific groups are shown together for inventory, not as a recommended configuration for one renderer. Some become required/invalid depending on backend.

```elixir
[
  # Session / rendering
  otp_app: :my_app,             # inferred by viewport; required by EmergeSkia.start
  backend: :wayland,           # platform default; :macos on macOS
  rendering_api: :auto,
  owner: nil,                  # low-level native option; managed by viewport

  # Window / dimensions / input
  title: "Emerge",
  width: 800,
  height: 600,
  always_on_top: false,        # macOS
  scroll_line_pixels: 30.0,

  # DRM
  drm_card: nil,               # effective default: /dev/dri/card0
  drm_output: nil,             # automatic connector selection
  drm_mode: nil,               # automatic; explicit mode requires drm_output
  vulkan_drm_node: nil,        # required for explicit DRM Vulkan
  drm_startup_retries: 40,
  drm_retry_interval_ms: 250,
  drm_force_gpu_finish: false,
  hw_cursor: true,
  drm_cursor: [],              # default/text/pointer: [source: ..., hotspot: {x, y}]

  # Diagnostics
  input_log: false,
  render_log: false,
  close_signal_log: false,
  stats: false,
  renderer_stats_log: false,
  renderer_animation_log: false,

  renderer_cache: [
    enabled: true,            # defaults false for explicit raster rendering
    max_new_payloads_per_frame: 64,
    paint_layer: [
      max_entries: 512,
      max_bytes: 671_088_640,
      max_entry_bytes: 268_435_456,
      min_visible_before_store: 1,
      max_stale_frames: 120
    ]
  ],

  assets: [
    fonts: [],                # [family: ..., source: ..., weight: 400, italic: false]
    decode_at_size: false,
    runtime_paths: [
      enabled: false,
      allowlist: [],
      follow_symlinks: false,
      max_file_size: 25_000_000,
      extensions: [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp", ".svg"]
    ],
    cache: [
      max_entries: 256,
      max_bytes: 268_435_456,
      svg_tree_max_entries: 64,
      svg_tree_max_bytes: 16_777_216
    ]
  ],

  headless: [
    mode: :binary,            # or :prime
    target: nil,             # live local PID required for headless
    pixel_format: :rgba8888,
    bw1_polarity: :one_is_black,
    dither: false,
    target_fps: nil,
    frame_message: :emerge_skia_frame,
    prime: [
      drm_node: nil,
      max_in_flight: 2,
      on_backpressure: :drop_new
    ]
  ],

  viewport: [
    input_mask: nil,
    renderer_check_interval_ms: 500,
    renderer_module: Emerge.Runtime.Viewport.Renderer.Skia,
    renderer_opts: []
  ]
]
```

Additional supported forms:

- `backend`: `:macos | :wayland | :drm | :headless`.
- `rendering_api`: `:auto | :metal | :opengl | :raster | :vulkan`, subject to backend/build support; also `[raster: [present: :auto | :cpu | :gpu_upload]]` and `[auto: [raster: [present: ...]]]`.
- `drm_mode`: advertised mode map or its exact ID. `drm_cursor` can override any of `:default`, `:text`, `:pointer`; each entry needs a source and hotspot. Font list entries require family/source, with optional weight/italic defaults above.
- Deprecated compatibility names: `backend_renderer` for `rendering_api`, `:gl` for `:opengl`.
- `name`, `timeout`, `debug`, `spawn_opt`, `hibernate_after` passed to viewport `start_link/1` are GenServer startup options, not mount/display settings.

## Proposed struct model

Replace the scattered keyword configuration with `%Emerge.Config{}` composed of meaningful domain structs. Do not merely wrap the flat options in one giant struct. Struct names/fields below are a design proposal, not implemented APIs.

```elixir
alias Emerge.Config

backlight = %Config.Backlight{device: :auto, brightness: 0.7}

output = %Config.DRM{
  card: "/dev/dri/card0",
  connector: "DSI-1",
  mode: nil,
  display: :on,
  backlight: backlight
}

renderer = %Config.Renderer{api: :raster, target_fps: 30}

%Config{output: output, renderer: renderer}
```

Other groups have defaults; applications need not spell out every nested struct. The root has six responsibility groups:

| Root field | Struct / responsibility |
|---|---|
| `output` | Exactly one `%Config.Window{}`, `%Config.DRM{}` or `%Config.Headless{}`: where frames go. |
| `renderer` | `%Config.Renderer{}`: rendering API, GPU selection, raster presentation route, frame-rate cap and paint caching. |
| `input` | `%Config.Input{}`: typed raw-event subscriptions, wheel distance and cursor configuration. |
| `assets` | `%Config.Assets{}`: application asset root, fonts, runtime path policy, decoding and asset caches. |
| `diagnostics` | `%Config.Diagnostics{}`: stats, selected log categories and advanced diagnostics. |
| `runtime` | `%Config.Runtime{}`: advanced health/retry policy and custom adapter configuration, normally left at defaults. |

### Output variants

```elixir
# Desktop window; platform backend can be automatic or explicit.
%Config.Window{
  backend: :auto,            # :auto | :wayland | :macos
  title: "Emerge",
  size: {800, 600},
  always_on_top: false
}

# Physical DRM output; type selects DRM, no duplicate backend field.
%Config.DRM{
  card: "/dev/dri/card0",
  connector: "DSI-1",
  mode: nil,                # automatic or advertised exact mode ID/map
  preferred_size: nil,      # optional selection hint, not a framebuffer size
  display: :on,             # :on | :off
  backlight: %Config.Backlight{device: :auto, brightness: nil}
}

# Headless output; no unrelated window, connector or backlight fields.
%Config.Headless{
  size: {800, 600},
  target: frame_sink,
  frame_message: :emerge_skia_frame,
  frames: %Config.BinaryFrames{pixel_format: :rgba8888}
}
```

`Headless.frames` is either `%Config.BinaryFrames{pixel_format, bw1_polarity, dither}` or `%Config.PrimeFrames{device, max_in_flight, on_backpressure}`. This removes an independent mode flag and inactive PRIME settings from binary configuration. Window backend selection remains explicit where needed; output type selects DRM/headless without redundant root `backend` values.

### Mapping existing options by responsibility

| Existing options | Proposed location |
|---|---|
| `backend` | Output struct type; `Window.backend` chooses desktop backend. |
| `title`, `always_on_top` | `output` when it is a `Window`. |
| `width`, `height` | `Window.size`, `Headless.size`, or `DRM.preferred_size` depending on their actual meaning. |
| `drm_card`, `drm_output`, `drm_mode` | `DRM.card`, `DRM.connector`, `DRM.mode`. |
| New display/backlight control | `DRM.display`, `DRM.backlight.device`, `DRM.backlight.brightness`. |
| `rendering_api`, nested raster presentation | `renderer.api`, `renderer.raster_present`. |
| `vulkan_drm_node` | `renderer.device`; initially supported for explicit Vulkan selection, still independent of the KMS card. |
| `headless.target_fps` | `renderer.target_fps` for all supported output types; physical refresh remains a DRM mode property. |
| `renderer_cache` | `renderer.cache`, using renderer/paint-layer cache structs. |
| `viewport.input_mask`, `scroll_line_pixels` | `input.events` (atom list, mask stays internal), `input.scroll_line_pixels`. |
| `hw_cursor`, `drm_cursor` | `input.cursor`: hardware preference and typed per-icon overrides; retain backend capability validation. |
| `otp_app`, `assets.*` | `assets.otp_app`, typed font entries, runtime-path policy and asset-cache structs. |
| Logging booleans | `diagnostics.logs`, a list of categories (`:input`, `:render`, `:close`, `:stats`, `:animation`) instead of unrelated root flags. |
| `stats`, `drm_force_gpu_finish` | `diagnostics.stats`, `diagnostics.force_gpu_finish` (still an advanced DRM/OpenGL diagnostic). |
| DRM retry options, renderer health interval | `runtime.startup_retries`, `runtime.retry_interval_ms`, `runtime.health_check_interval_ms`; preserve existing backend-specific behavior during migration. |
| `viewport.renderer_module`, `viewport.renderer_opts` | `runtime.adapter`, `runtime.adapter_opts`; opaque options remain allowed for custom adapters. |
| `owner` | Not in viewport config. Lifecycle-owned; retain any necessary standalone low-level API separately. |
| GenServer startup options | Remain process startup options, outside `%Config{}`. |

Cache budgets, font entries, cursor overrides and filesystem policy deserve typed nested structs. Do not add wrapper structs around every scalar. Specify exact fields/defaults intentionally before implementation; choose them for the 0.5 API, not to preserve accidental behavior of the old keyword layout.

### Input subscriptions and public types

Replace the numeric user-facing mask with an explicitly typed list. Proposed category names:

```elixir
%Config.Input{
  events: [:key, :text, :pointer_button, :resize],
  scroll_line_pixels: 30.0
}

# In Emerge.Config.Input:
@type event_kind ::
        :key
        | :text
        | :pointer_move
        | :pointer_button
        | :pointer_scroll
        | :pointer_enter
        | :resize
        | :focus

@type t :: %__MODULE__{
        events: [event_kind()],
        scroll_line_pixels: number(),
        cursor: Emerge.Config.Cursor.t()
      }
```

- No integers, `nil`, strings or arbitrary atoms in the new `events` field. Reject unknown atoms with an error listing allowed values; deduplicate entries and treat order as insignificant when comparing config.
- `[]` disables raw input delivery to `handle_input/2`. `Config.Input.all_events/0` returns all eight categories as a list; no extra `:all` / `:none` sentinel values are necessary. Default remains a design decision: recommend `[]` for opt-in raw observation. The earlier all-events default was only a compatibility proposal; native interactions do not depend on raw callback subscriptions.
- This controls the **raw observer callback only**. It does not disable native input processing, hit testing, scrolling, text editing or element events routed through `handle_info/2`. Window close remains `handle_close/2`, not a selectable raw input event.
- Live config changes replace subscriptions. Already-forwarded messages may still be in the mailbox; applying new subscriptions does not retract them. Replay subscriptions on renderer recovery. Conversion to a numeric mask belongs at the native/transport boundary, not application code.
- Callback payloads are not constrained by backward compatibility either. Settle a coherent typed 0.5 contract, including subscription/event naming and readable action values, before implementation. Prefer consistent pointer terminology and typed `:press | :release` over historical numeric actions where appropriate. The misleading `codepoint` category becomes `:text`, including composition and surrounding-text requests.

The table below audits the **existing 0.4 first argument of `handle_input/2`**, not the low-level `{:emerge_skia_event, event}` envelope. It is not the final 0.5 payload contract. The new public docs must provide an equally complete table for the chosen 0.5 types:

| Subscription | What it observes | Existing callback payload(s) |
|---|---|---|
| `:key` | Canonical key press/release, not inserted text | `{:key, {key, action, modifiers}}` |
| `:text` | Committed text and IME composition/edit requests | `{:text_commit, {text, modifiers}}`; `{:text_preedit, {text, cursor_range_or_nil}}`; `:text_preedit_clear`; `{:delete_surrounding, {before_bytes, after_bytes}}` |
| `:pointer_move` | Cursor position changes | `{:cursor_pos, {x, y}}` |
| `:pointer_button` | Pointer button press/release | `{:cursor_button, {button, action, modifiers, {x, y}}}` |
| `:pointer_scroll` | Normalized scrolling at a cursor position | `{:cursor_scroll, {{dx, dy}, {x, y}}}` |
| `:pointer_enter` | Enter/leave of the window, not an element hover | `{:cursor_entered, entered?}` |
| `:resize` | Output dimensions/scale changes | `{:resized, {width, height, scale}}` |
| `:focus` | Window focus changes, not focused UI elements | `{:focused, focused?}` |

Audit sources: `native/emerge_skia/src/input.rs` (masks, acceptance and encoding), `events/runtime.rs` (separate observer/listener paths), `lib/emerge_skia/macos/{host,protocol,session}.ex` (host decoding/filtering) and viewport routing in `lib/emerge/runtime/viewport.ex`.

Documentation/type requirements:

- Add public `Emerge.Input.event()` plus named types for every payload, position, scroll delta, action, modifier, button and canonical key. Change `Emerge.handle_input/2` from `term()` to this event union. Do not use `atom()` for closed enumerations.
- Current action values are `0` (release) / `1` (press); modifiers are `:shift | :ctrl | :alt | :meta`; buttons are `:left | :right | :middle | :back | :forward | :other`. Audit key/repeat behavior and enumerate keys from the canonical key definitions rather than inventing another mapping.
- Every configurable atom enum across the new config structs gets a named union type and `@typedoc`, including display state, API/backend, frame format, diagnostics categories and backpressure policy. Document scalar fields/defaults too. Open-ended module identifiers/custom adapter data are not closed enums.
- Put the full subscription table, default/empty/all behavior and representative `handle_input/2` clauses in `Emerge.Config.Input` ExDoc, with links from `Emerge.handle_input/2`, the viewport tutorial and input event types. Add a discoverable user-facing input guide; do not bury the contract only in internals or mask helper docs.
- Audit/document units and signs (coordinates, scroll deltas, dimensions, scale), text versus keys, repeat semantics, IME cursor-range units, window versus element focus/enter, and startup resize/focus delivery. Do not assume native/host paths have identical units without verification.
- Explicitly document backend availability: choosing a category does not create hardware events. Headless has no physical input source; DRM is not a desktop window; IME support varies. In particular, the audited macOS host routes commit/preedit/clear but has no delete-surrounding notification handler in that dispatch section; do not promise every `:text` variant on every backend.

Acceptance: ExUnit covers all category-to-mask mappings, empty/all/duplicate/invalid lists, order-insensitive diffs, defaults, live replacement and recovery. Event fixtures validate actual callback payloads against the docs/types on native and macOS transports. Verify disabled raw subscriptions leave element events and native text/scroll behavior intact. Check ExDoc links/examples and prevent documented enum values from drifting from validation. Remove public numeric-mask helpers/config/setters; retain bitmasks only as a private transport/native implementation detail if useful. Test rejection of removed public config forms instead of legacy conversion.

### Defaults and nested keyword shorthand

Prefer ordinary struct syntax, not a public `Config.new`/`Config.update` or constructor family. Every config struct supplies documented defaults, including nested structs. `%Config{}` describes the ordinary default viewport; users specify only deviations, without spelling out empty Input/Diagnostics/cache structs.

```elixir
%Config{output: %Config.Window{title: "My App"}}

%Config{
  output: %Config.DRM{
    connector: "DSI-1",
    backlight: [brightness: 0.7]
  },
  renderer: [api: :raster, target_fps: 30],
  input: [events: [:key, :resize]]
}
```

- Root config and ambiguous variants (Window/DRM/Headless output; Binary/Prime frames) use explicit structs. Fixed-shape nested records accept either their struct or a documented keyword shorthand. The enclosing field determines the type; no runtime guessing or generic map conversion.
- At the runtime boundary, normalize keyword shorthand into canonical nested structs and fill omitted values from that record's defaults. `%Config.DRM{backlight: [brightness: 0.7]}` therefore normalizes to a DRM config containing `%Config.Backlight{device: :auto, brightness: 0.7}`. No hardware access is needed for structural normalization.
- Previous config passed to the callback is always normalized: nested records are structs, not a mixture of keywords/structs. Prefer explicitly typed struct update syntax (`%Config{previous | ...}` and named nested struct updates) to change it. This is the syntax used in public documentation and examples. Use named intermediate variables to update one config struct at a time, then assemble the parent; avoid a single deeply nested struct-update expression. Other standard Elixir update operations remain valid. Updating only brightness this way preserves the selected device and connector.
- A supplied nested record replaces that record; shorthand fills missing fields from defaults, not from previous config. This follows ordinary struct-update semantics. Use `%Config.Backlight{previous.output.backlight | brightness: value}` when preservation is intended. A returned fresh `%Config{}` means its actual defaults; there is no implicit mount-baseline overlay.
- Collection fields (`events`, fonts, allowlists, log categories) retain their documented collection types and replace rather than append/merge. Explicit `nil` uses the field's documented meaning, not a generic deletion marker.
- Types must expose both accepted shorthand and canonical forms: e.g. `Backlight.options()` enumerates `{:device, device()}` and `{:brightness, brightness()}` entries, and the public field accepts `Backlight.t() | Backlight.options()`. Do not hide legal keyword fields behind an untyped `keyword()`. Document the normalized previous-config guarantee and reflect it in callback argument types.
- Validate unknown keyword keys, wrong nested structs and invalid values at normalization, with full field paths. Ordinary struct construction does not execute a constructor or validate nested keyword values. Infer context-dependent values (e.g. viewport asset application) and check runtime requirements at startup; do not fabricate a headless recipient or Vulkan device.
- This is a new nested shorthand within typed config, not acceptance of the old flat config/namespaces. No constructor/update API is needed.
- Tests cover defaults, minimal literals, shorthand/explicit-struct equivalence, idempotent normalization, normalized callback arguments, standard nested updates preserving other fields, record/collection replacement, invalid keys/types and non-mutation.

### Callback semantics with previous config

Both mount and config return `%Config{}`. Proposed callback signatures:

```elixir
@callback config(Emerge.Config.normalized_t()) :: Emerge.Config.t()
@callback config(Emerge.Config.normalized_t(), state()) :: Emerge.Config.t()
```

- Implement neither callback (mount-only behavior), or exactly one arity. `config/1` receives previous config for a stateless viewport; `config/2` receives previous config first, then application state. No `config/0`, because every invocation must have access to previous config. This replaces the earlier state-only callback proposal.
- First invocation receives normalized mount config with defaults filled in. Subsequent invocations receive the latest validated, normalized desired config. Fixed-shape nested records are always structs in this argument. Applications need not duplicate it in state. Callback/validation failure leaves that previous config intact.
- Previous means **desired**, not last physically applied: asynchronous KMS work, renderer downtime, temporary darkening or unresolved device defaults must not overwrite application intent. Recovery retains the latest desired config instead of resetting to mount values.
- The callback returns a complete root config, optionally with nested keyword shorthand. Explicitly typed struct updates preserve untouched previous fields; newly supplied records use their own defaults. There is no implicit merge of a returned record with previous or mount config.
- Evaluate config before render during the existing coalesced cycle, including initial startup and while display is off. Keep callbacks in the viewport process. Normalize/validate and compare the result; shorthand and equivalent explicit structs cause no spurious changes. Startup-only changes are still rejected during reconciliation.

```elixir
@impl Viewport
def config(previous, state) do
  backlight = %Config.Backlight{previous.output.backlight | brightness: state.brightness}
  output = %Config.DRM{previous.output | display: state.display, backlight: backlight}

  %Config{previous | output: output}
end
```

Store resolved hardware defaults separately from declarative structs. The runtime must not replace desired brightness with temporary zero or the desired display state with an effective disabled state.

### Display / backlight semantics

- `DRM.display` remains `:on | :off`. Off disables scanout/backlight and avoids unnecessary UI work without changing desired brightness. On presents a current frame before illumination. No wake-button, suspend or application power-state concept.
- `Backlight.device`: `:auto` requires unambiguous association, `nil` opts out, a path binds explicitly. Selection is startup-only initially; automatic association is revalidated on connector changes.
- `Backlight.brightness`: finite normalized `0..1`; zero is not display-off. `nil` means use the device's initial requested level as a resolved session baseline. Capture it before darkening and retain it across recovery; never reinterpret Emerge's temporary zero as the baseline.
- An explicit unavailable device or brightness request without a controllable backlight reports an error; no device/no request permits KMS-only operation. Unsupported backend combinations are explicit errors, not silent success.
- Physical display/backlight control belongs to DRM output configuration, not desktop-wide monitor control under Window.

### 0.5.0 breaking migration

- Root config is a struct, with typed nested keyword shorthand where the record type is unambiguous. Remove old flat keyword config/namespaces and deprecated aliases rather than converting or warning-and-accepting. Application-specific `mount` arguments and OTP process-start options remain separate.
- Update public renderer start and adapter contracts to the new model; keep any native wire-format encoding private. No legacy adapter bridge or public numeric-mask API is required.
- Input payload naming/types and defaults can change for clarity; there is no requirement to preserve historical tuples or numeric actions. Finalize that contract explicitly rather than accidentally changing it while implementing the atom list.
- Update tests/examples and docs for the new API, with a 0.4-to-0.5 migration guide and changelog. Perform the version/artifact metadata bump during implementation, not this planning task. Coordinate standalone demo updates.
- Confirm remaining struct/field naming, full-config semantics and input payloads/defaults before implementation. The compatibility decision is settled: none for the replaced APIs. No source changes are authorized by this proposal.
