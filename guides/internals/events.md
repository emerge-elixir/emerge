# Events System

This document describes the current retained-mode event architecture for
EmergeSkia.

## Overview

- Rust owns hit testing, pointer state, hover state, click detection, scroll
  request generation, interaction-style activation (`mouse_over`, `focused`,
  `mouse_down`), and focused text-input editing state.
- Elixir owns payload routing (`{pid, msg}`) keyed by encoded `element_id`.
- EMRG encodes event attributes as presence flags only (no payloads).
- Scrollbar-specific hit testing and interaction state live in
  `native/emerge_skia/src/events/scrollbar.rs` and are coordinated by
  `EventProcessor`.
- Wayland text input is IME-aware (`Ime::Preedit` + `Ime::Commit`), while DRM
  currently emits simple text commits from key mapping.

## End-to-End Event Flow

```
Backend input (Wayland/DRM)
  -> InputEvent
  -> Event actor
       -> sends raw input event to target pid
       -> EventProcessor uses registry for hit testing
            -> emits element event {:emerge_skia_event, {element_id_bin, event_atom}}
  -> Elixir looks up element_id_bin + event_atom
  -> Elixir dispatches stored {pid, msg}
```

Notes:

- Raw input events and element events are both delivered as
  `{:emerge_skia_event, ...}`.
- `element_id_bin` is the `term_to_binary` payload for the element id.
- Element events may include payloads (for example, text input change events
  include the latest string value).

## Event Registry

After each tree upload, patch, scroll-driven update, or asset-state update,
Rust rebuilds the event registry from the current tree.

Each node stores:

- target id
- hit rectangle
- event flags
- self rounded-corner data
- active clip rectangle and clip rounded-corner data

Registry order follows render traversal order. Hit testing scans in reverse, so
topmost elements win.

## Nearby Status

Nearby positioning is currently visual-only.

- nearby subtrees are rendered from nested EMRG attr payloads during the render pass
- nearby nodes are not yet inserted into the retained event registry walk
- current hit testing, hover, focus, and text-input runtime only see main-tree
  `children`
- direct-listener-registry work should treat nearby as first-class retained mounts
  and preserve the same ordering described in `nearby-semantics.md`

## Hit Testing Behavior

Current hit testing is:

- clip-aware (including inherited clip intersections)
- padding-aware for clip regions
- rounded-corner-aware (self and active clip)
- scroll-offset-aware for descendants of scrollable containers

Flag filtering happens before geometric checks, so non-listener nodes do not
block listeners behind them.

## Click, Hover, and Button Behavior

- `on_click` is emitted on left-button press+release on the same element.
- `on_press` uses the same left-button press+release activation as `on_click`.
- `on_press` is also emitted when a focused pressable element receives `Enter`.
- `on_mouse_down` and `on_mouse_up` are emitted for left button only.
- Hover state tracks topmost hit and emits `:mouse_enter`, `:mouse_leave`, and
  `:mouse_move` based on listener flags.
- Focusable inputs emit `:focus` when they gain focus and `:blur` when focus leaves.
- A drag deadzone suppresses click when pointer movement exceeds the threshold
  during a press.

## State Styling Behavior

- `mouse_over` is a style attribute, not an emitted element event.
- EventProcessor hit tests for the topmost node with `mouse_over` and sends
  tree requests as the active element changes.
- Tree updates use `TreeMsg::SetMouseOverActive { element_id, active }`.
- `mouse_down` is also style-only; EventProcessor toggles runtime state with
  `TreeMsg::SetMouseDownActive { element_id, active }` on left press/release.
- `focused` is style-only and is activated from runtime focus state
  (`focused_active`) on focused inputs.
- Layout merges active style blocks in this order: `mouse_over -> focused ->
  mouse_down`. Later styles win on attribute conflicts.
- Supported decorative attrs in state styles are: `background`,
  `border_color`, `box_shadow`, `font_color`, `font_size`,
  `font_underline`, `font_strike`, `font_letter_spacing`,
  `font_word_spacing`, `move_x`, `move_y`, `rotate`, `scale`, and `alpha`.

## Scroll-Related Event Behavior

- Wheel and drag scrolling both use the same registry.
- Arrow-key scrolling also uses registry matchers.
- Directional scroll flags are computed from current offset vs max offset.
- EventProcessor converts pointer movement/wheel deltas and keyboard navigation
  into scroll requests to the tree actor.
- Scrollbar track/thumb hit testing and thumb drag are implemented (track click
  snaps thumb to cursor, then drag continues from that point).
- Scrollbar hover emits axis-specific hover updates for thumb styling.
- After scroll changes, layout/render output and event registry are refreshed to
  keep hit testing aligned with visible content.

## Asset-Triggered Refreshes

- Image assets load asynchronously in Rust (`AssetManager` actor).
- Asset completion/failure sends `TreeMsg::AssetStateChanged`.
- Tree actor reruns layout/render and pushes a fresh event registry so hit bounds
  stay aligned with final image geometry.

## Elixir Responsibilities

- Build and maintain `%{element_id_bin => %{event => {pid, msg}}}` in diff
  state.
- Encode event attrs as presence flags in EMRG (`on_click`, `on_mouse_*`,
  `on_press`, `on_change`, `on_focus`, `on_blur`).
- Encode `mouse_over`, `focused`, and `mouse_down` as typed decorative attr
  blocks (no payload routing).
- On Rust element events, resolve and forward stored payloads.

## Supported Element Events

- `:click`
- `:press`
- `:mouse_down`
- `:mouse_up`
- `:mouse_enter`
- `:mouse_leave`
- `:mouse_move`
- `:change` (text input, payload includes latest value; emitted only when `on_change` is set)
- `:focus` (focusable inputs)
- `:blur` (focusable inputs)

`mouse_over`, `focused`, and `mouse_down` do not emit element events; they are
applied as runtime styling in Rust.

## Raw Text Input Events

Backends send raw text input events to the configured input target process:

- `{:text_commit, {text, modifiers}}`
- `{:text_preedit, {text, cursor_range}}`
- `:text_preedit_clear`

Text commit events mutate focused text-input content in Rust. Preedit events
track composition state for IME workflows and do not emit `:change` by
themselves.

Text editing remains active without `on_change`; `on_change` gates only
whether `:change` element events are emitted.

## Text Selection and Clipboard Shortcuts

- Selection is tracked in Rust runtime attrs (`cursor` + `selection_anchor`) and
  is not encoded in EMRG.
- Mouse drag selects text within focused single-line inputs.
- Tab cycles focus across all rendered focusable inputs (including clipped
  descendants).
- Shift+Tab cycles focus in reverse order.
- Focus changes can emit registry-derived scroll requests so the focused element
  is brought into view.
- If left/right/home/end cannot move a focused text cursor (already at bound),
  the key can fall back to ancestor scrolling via registry matchers.
- If no focused directional matcher is available, arrow keys fall back to the
  first visible root-first scrollable that can scroll in that direction.
- Shift+arrow/home/end extends selection.
- Ctrl/Meta shortcuts are handled in Rust for focused text inputs:
  - `A` select all
  - `C` copy selection
  - `X` cut selection
  - `V` paste text
- Linux PRIMARY selection is tracked separately and updated from current
  selection/copy/cut.
- Middle mouse button pastes from Linux PRIMARY selection into focused text
  inputs.
- Cut, paste, and typed insertion replace the selected range when present and
  emit `:change` with updated value when `on_change` is set.

## Current Limits

- No bubbling/capture propagation.
- No double-click semantics.
- Element events do not include pointer metadata payloads.
- Right/middle buttons are not mapped to element-level down/up events.
- No distinct scrollbar active/pressed visual state beyond hover width changes.
- Nearby elements are not yet hit-testable or focusable through the event registry.

## Possible Extensions

- Optional metadata payloads for element events (position/modifiers/button).
- Optional bubbling/capture model.
- Optional multiple input targets.
- Multi-touch pointer ids and gesture hooks.
