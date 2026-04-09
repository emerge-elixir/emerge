# Events System

This document describes the current event architecture used by EmergeSkia.

## Overview

EmergeSkia keeps event state in the retained UI tree and dispatches input by
matching it against listeners rebuilt from that tree in precedence order.

- The tree actor owns tree mutation, layout, rendering, and `base_registry`
  rebuilds.
- The event actor owns listener dispatch, runtime overlay state, and input
  buffering while listener data is stale.
- Listener dispatch is
  `InputEvent -> ListenerInput -> first matching listener in precedence order -> ordered ListenerAction list`.
- The rebuild output is generated during the same main-tree walk that produces
  the render scene.

At a high level:

- Rust owns hit testing, hover/focus/button state activation, scroll behavior,
  scrollbar behavior, and focused text-input runtime.
- Elixir owns payload routing for emitted element events.

## Actors And Responsibilities

### Tree Actor

The tree actor:

- receives `TreeMsg` mutations
- applies and coalesces those mutations against the retained tree
- runs `layout_and_refresh_default(...)` when the tree actually changed
- produces:
  - render scene
  - IME state
  - `RegistryRebuildPayload`
- sends `EventMsg::RegistryUpdate { rebuild }` to the event actor
- sends the render scene to the render thread

### Event Actor

The event actor:

- receives backend `InputEvent`s
- forwards raw observer input to the configured input target
- dispatches listener input against the current precedence order
- owns runtime overlay state such as:
  - click/press trackers
  - drag trackers
  - scrollbar drag trackers
  - text-drag trackers
- buffers listener-lane input while listener data is stale
- installs fresh registry rebuilds from the tree actor
- rebuilds overlay listeners from runtime state after registry changes

### Render Thread

The render thread only consumes the render scene and IME state. It does not perform
event matching.

## End-To-End Flow

```text
Backend input
  -> InputEvent
  -> Event actor
       -> observer lane forwards raw input (mask-gated)
       -> listener lane dispatches current precedence order
       -> matched listener emits ordered actions
            -> TreeMsg batch
            -> Elixir element event
            -> runtime state mutation
  -> Tree actor
       -> drains and flattens queued TreeMsg batches
       -> applies/coalesces tree mutations
       -> if tree changed: layout + render + rebuild base listener data
       -> if only registry rebuild was requested: may reuse cached rebuild
       -> sends EventMsg::RegistryUpdate { rebuild }
       -> sends RenderMsg::Scene
  -> Event actor
       -> installs rebuild
       -> reconciles retained rebuild state
       -> rebuilds overlay listeners
       -> replays buffered listener-lane input
```

## Listener Data

The event system uses two listener registries:

- `base_registry`
  - built by the tree actor from the retained tree during refresh
- `overlay_registry`
  - rebuilt by the event actor from transient runtime state

Dispatch uses one logical precedence order:

- overlay listeners outrank base listeners
- later-painted children outrank parents and earlier siblings
- within one element, builder code is written in precedence order

The rebuild payload sent from tree actor to event actor contains:

- `base_registry`
- `text_inputs: HashMap<ElementId, TextInputState>`
- `scrollbars: HashMap<(ElementId, ScrollbarAxis), ScrollbarNode>`
- `focused_id`

## Precedence Order

Listener precedence matches visual precedence.

- The tree walk follows paint order.
- Children are accumulated after their parents.
- Later-painted descendants therefore outrank earlier siblings and parents.
- Within one element, listeners are assembled in explicit precedence order.

There is no bubbling, capture, or handler-side fallback chain. Precedence is
the only conflict rule.

## Listener Inputs

Listeners can match either backend input directly or derived internal input.

Current internal listener inputs are:

- `Raw(InputEvent)`
- `ScrollDirection`
- `PointerLeave`
- `PointerEnter`

This keeps one listener model while still allowing multi-step behavior such as
directional scroll dispatch and pointer lifecycle ordering.

## Dispatch

Dispatch is deterministic.

For one dispatch:

1. scan listeners in precedence order
2. first matching listener wins
3. compute its ordered actions
4. apply those actions in order
5. stop that dispatch

There is no bubbling, capture, or handler-side fallback chain.

## Splitter Listeners

Some behavior is implemented by top overlay listeners that turn one physical
input into a sequence of derived listener inputs.

### Scroll Splitter

Physical scroll input may contain both `dx` and `dy`.

The runtime installs a top overlay scroll splitter listener that:

- matches raw scroll input
- splits it into directional component inputs
- runs those derived inputs back through the base registry
- aggregates the returned actions

This lets one physical input resolve horizontal and vertical scrolling
independently while keeping normal first-match dispatch.

### Pointer Lifecycle Splitter

Pointer lifecycle ordering is preserved explicitly.

For pointer movement and left-button release inside the window, the splitter
runs:

1. synthetic `PointerLeave`
2. raw pointer input
3. synthetic `PointerEnter`

For window leave, it runs:

1. synthetic `PointerLeave`
2. raw window-leave input

This preserves leave -> raw -> enter semantics without separate runtime-specific
dispatch passes.

## Freshness, Rebuilds, And Buffered Input

The event runtime has two lanes:

- listener lane
- observer lane

Observer lane:

- forwards raw input immediately
- continues forwarding while the listener lane is stale

Listener lane:

- dispatches only while fresh
- buffers/coalesces input while stale
- resumes after `EventMsg::RegistryUpdate`

The listener lane becomes stale when a matched listener produces:

- at least one `TreeMsg`, or
- at least one Elixir element event

Runtime-only overlay mutations do not make listener data stale by themselves.

If a matched listener emits Elixir events but no tree messages, the event actor
adds `TreeMsg::RebuildRegistry` so the tree actor will still send a fresh
`RegistryUpdate`.

`EventMsg::RegistryUpdate` is the freshness signal. Installing it:

- replaces `base_registry`
- updates retained scrollbar/text rebuild state
- rebuilds `overlay_registry`
- replays buffered input if reconciliation did not immediately stale the lane
  again

## Tree Message Batching

The event actor groups tree messages emitted by one listener dispatch.

- one tree message is sent directly
- multiple tree messages are sent as `TreeMsg::Batch(Vec<TreeMsg>)`

The tree actor then:

- receives one message
- drains everything currently queued
- flattens any nested `Batch(...)`
- processes one flat message list

Tree-side coalescing happens before refresh:

- `ScrollRequest` values accumulate per element
- scrollbar thumb drag deltas accumulate per element
- hover and active state messages are last-write-wins per element
- text content/runtime updates are applied in message order

This is why one listener producing several tree-side actions normally causes one
tree refresh, not one refresh per message.

### Cached Registry Rebuilds

The tree actor caches the latest `RegistryRebuildPayload`.

If it receives only `TreeMsg::RebuildRegistry` and the tree did not actually
change, it can resend the cached rebuild to the event actor without rerunning
layout/render/rebuild.

If no cached rebuild exists yet, it falls back to a full refresh.

## Hit Testing And Interaction Geometry

Hit testing uses `tree::interaction` as the source of truth.

Current matching is:

- clip-aware
- rounded-corner-aware
- scroll-aware
- based on retained interaction geometry computed before refresh

Refresh begins with an interaction pre-pass:

- `populate_interaction(tree)`

The combined render/rebuild walk then consumes that interaction data for both
drawing and listener accumulation.

## Pointer Behavior

### Click And Press

Pointer `on_click` and pointer `on_press` share the same left-button tracker
flow:

- left press starts:
  - click/press tracker
  - drag tracker candidate
- drag threshold promotion drops the click/press tracker
- release followups are looked up again from the current base registry

`on_press` also has focused keyboard `Enter` support.

Pointer swipe listeners (`on_swipe_up`, `on_swipe_down`, `on_swipe_left`,
`on_swipe_right`) reuse the same press-origin drag threshold flow:

- left press starts a drag tracker candidate
- once movement clears the deadzone, runtime waits for a dominant axis intent
- if vertical or horizontal intent is clear, scroll gets first claim on that axis only
- if no scroll listener matches the locked axis, runtime promotes to swipe tracking instead
- active drag-scroll and swipe tracking both stay axis-locked until release
- swipe direction is decided on release from final net displacement along the locked axis
- short locked-axis displacement does not emit a swipe event

Focused elements can also register direct keyboard listeners:

- `on_key_down`
- `on_key_up`
- `on_key_press`

Keyboard listener matchers use canonical key atoms such as `:enter`, `:a`,
`:digit_1`, `:arrow_left`, and `:plus`, plus modifier filters drawn from
`[:shift, :ctrl, :alt, :meta]`.

`on_key_press` is release-based: it arms on matching key-down and completes on
matching key-up. `on_key_up` runs before `on_key_press` completion.

Raw key events stay canonical and layout-independent. Text input still arrives
through text commit events, so `Shift+=` matches raw key `:equal` with `:shift`
while committing the text `"+"`.

### Mouse Down / Mouse Up

- `on_mouse_down` is left-button only
- `on_mouse_up` is left-button only
- `on_mouse_up` remains targeted to the release location
- some style-clear behavior is release-anywhere, leave, or blur-driven

### Hover

Hover tracking is listener-driven and based on retained hover state.

- event-only hover and style hover both retain hover-active state
- hover enter/leave listeners are built from current retained state
- `:mouse_enter`, `:mouse_leave`, and `:mouse_move` are emitted only when the
  corresponding listener flags exist
- pointer lifecycle transitions are resolved as leave -> raw -> enter
- raw move handles hover transitions while the pointer stays inside the same
  element

Scrollbar thumb hover is element-local:

- raw move computes thumb enter/leave while the pointer remains inside the
  element
- element leave clears any active scrollbar hover
- there are no separate scrollbar enter/leave dispatch passes

### Style Activation

These are style-only tree/runtime states, not element events:

- `mouse_over`
- `mouse_down`
- `focused`

They are activated by tree messages such as:

- `TreeMsg::SetMouseOverActive`
- `TreeMsg::SetMouseDownActive`
- `TreeMsg::SetFocusedActive`

Layout then merges active style blocks in this order:

- `mouse_over`
- `focused`
- `mouse_down`

Later style layers win on conflicts.

State style blocks currently support decorative attrs from these categories:

- background
- border: `border_radius`, `border_width`, `border_style`, `border_color`, `box_shadow`
- font: `font`, `font_weight`, `font_style`, `font_size`, `font_color`, `font_underline`, `font_strike`, `font_letter_spacing`, `font_word_spacing`, `text_align`
- svg tint: `svg_color`
- transforms: `move_x`, `move_y`, `rotate`, `scale`, `alpha`

Because these styles are merged before measurement and resolution, layout-affecting
decorative attrs such as `border_width` and `text_align` participate in the
same frame as the active interaction state.

## Scroll Behavior

Wheel, drag-scroll, and key-scroll all use the same directional availability
model.

### Directional Listener Registration

A scrollable element only registers listeners for directions it can currently
satisfy:

- `x-` if `scroll_x > 0`
- `x+` if `scroll_x < scroll_x_max`
- `y-` if `scroll_y > 0`
- `y+` if `scroll_y < scroll_y_max`

If an element cannot scroll in a direction, it does not register a listener for
that direction at all.

This is what allows propagation to parents naturally: if the child has no
matching listener for that direction, the parent can win through normal
precedence order.

### Wheel And Trackpad Scroll

Physical scroll input may contain both `dx` and `dy`.

The runtime handles that by installing a top overlay scroll splitter listener:

- it matches raw scroll input
- splits it into directional component inputs
- runs those derived inputs back through the base registry
- aggregates the returned actions

This preserves first-match dispatch while letting one physical input resolve
both horizontal and vertical scrolling independently.

### Drag Scroll

Drag scroll uses the same path as wheel scroll after drag activation.

- pointer movement becomes synthetic scroll input
- that synthetic input is split into directional components
- those components are run back through the base registry

So drag scroll and wheel scroll follow the same propagation and precedence
rules.

### Key Scroll

Arrow-key scrolling is ordinary per-element listener matching in the base
registry.

It uses the same directional availability rules as wheel and drag scroll.

There is no separate fallback key-scroll subsystem.

### Scrollbar Hover And Drag

Scrollbar behavior is also listener-driven.

- scrollbar hover is computed by the element's raw move listener
- element leave clears active scrollbar hover
- thumb or track press starts runtime scrollbar drag state
- thumb drag emits `ScrollbarThumbDragX` / `ScrollbarThumbDragY`
- release clears scrollbar drag state

Scrollbar press listeners are more specific than the generic element-wide
left-press listener, so scrollbar drag starts from the scrollbar region rather
than from generic click/drag bootstrap.

### Scroll State And Clamping

Scroll state is owned in Rust.

Per axis, retained state includes:

- offset
- max offset

Clamping rules:

- offsets are clamped to `[0, max]`
- if max shrinks, offset clamps toward start
- if max grows and previous offset was at end, end anchoring is preserved
- if a scrollbar axis is disabled, that axis offset and max are cleared

### Scroll Rendering Notes

Rendering applies scroll state directly:

- child content renders under translated scroll offset
- clip rects are padding-aware
- scrollbar thumb geometry is derived from viewport/content ratio and current
  offset
- hover is axis-specific and widens thumb thickness for the hovered axis

## Focus Behavior

Focusable elements are not limited to text inputs.

Current focusable behavior includes elements that are:

- text inputs
- pressable
- explicitly focus-listening (`on_focus`, `on_blur`)
- keyboard-listening (`on_key_down`, `on_key_up`, `on_key_press`)

### Focus Changes

Focus transitions are expanded into concrete actions such as:

- Tab / Shift+Tab traversal is handled on key-down, matching mainstream toolkit behavior
- `on_key_press` remains available for completed key gestures that should fire on release

- blur element event
- focus element event
- `SetFocusedActive`
- text-input runtime sync
- reveal scroll requests

Reveal scroll requests are precomputed during rebuild from retained scroll
contexts and emitted during the focus transition itself.

### Tab And Shift-Tab

Tab handling is global, but derived from paint-order focus entries gathered
during the rebuild walk.

Behavior is:

- no focused element:
  - `Tab` -> first painted focusable
  - `Shift-Tab` -> last painted focusable
- focused element:
  - `Tab` -> focusable painted after it
  - `Shift-Tab` -> focusable painted before it

Window blur clears focus through a window-level blur listener.

## Text Input State

Text input state is modeled with unified `TextInputState` for both single-line
and multiline inputs.

It contains both live editing state and layout metadata used for caret
hit-testing:

- content
- cursor
- selection anchor
- preedit text
- preedit cursor range
- focused flag
- `emit_change`
- text frame, alignment, and font metadata

### Editing Semantics

- text editing works regardless of `on_change`
- `on_change` only gates emitted `:change` element events
- typed insertion, backspace, delete, cut, paste, and selection replacement are
  handled in Rust
- multiline inputs insert newline on `Enter` by default and support wrapped
  caret movement and hit-testing
- matching `on_key_down` handlers can suppress default keydown-derived editing
  before the following text commit is applied
- focused cursor, selection, and preedit state are preserved across rebuilds
- focused runtime edit state remains the source of truth across rebuilds
- rebuild metadata refreshes geometry and style data used by later editing and
  caret hit-testing

### Selection And Clipboard

- selection is tracked by cursor + selection anchor
- mouse drag selects text in focused text inputs
- `Shift` with arrows/home/end extends selection
- multiline inputs also support `ArrowUp` / `ArrowDown` movement and line-based
  `Home` / `End`
- Linux PRIMARY selection is tracked separately
- middle mouse button pastes from PRIMARY
- copy/cut/update of PRIMARY selection happens in Rust runtime

## Observer Input And Elixir Responsibilities

Raw input forwarding is separate from listener dispatch.

Observer input:

- is forwarded to the configured input target
- is filtered by the current input mask
- continues while the listener lane is stale

Elixir responsibilities remain:

- build the `%{element_id_bin => %{event_atom => {pid, msg}}}` routing map
- encode event attrs as presence flags in EMRG
- receive Rust-emitted element events and forward stored payload routes

## Supported Element Events

Current emitted element events are:

- `:click`
- `:press`
- `:swipe_up`
- `:swipe_down`
- `:swipe_left`
- `:swipe_right`
- `:mouse_down`
- `:mouse_up`
- `:mouse_enter`
- `:mouse_leave`
- `:mouse_move`
- `:change`
- `:focus`
- `:blur`

`mouse_over`, `mouse_down`, and `focused` styling do not emit element events.
They are retained tree/runtime state used by layout/style merging.

## Nearby Status

Nearby now participates in the retained tree/event model.

- nearby mounts are retained on host elements, not decoded ad hoc during render
- layout computes nearby frames once and shares them with render, hit testing,
  and listener rebuilds
- nearby traversal order now participates in listener precedence and focus order
- nearby nodes are hit-testable, focusable, and preserve runtime state like
  normal retained nodes

## Current Limits

- no bubbling or capture propagation
- no double-click semantics
- no pointer metadata payloads on element events
- no right/middle element-level down/up events
- no multi-touch or gesture event model yet

## Key Files

- `native/emerge_skia/src/events.rs`
- `native/emerge_skia/src/events/registry_builder.rs`
- `native/emerge_skia/src/events/runtime.rs`
- `native/emerge_skia/src/lib.rs`
- `native/emerge_skia/src/tree/layout.rs`
- `native/emerge_skia/src/tree/render.rs`
