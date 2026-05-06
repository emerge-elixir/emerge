# Active Length Min/Max Combinators Plan

Last updated: 2026-05-06.

Status: implemented in the current working tree.

## Purpose

Make this layout shape work:

```elixir
height(min(content(), fill()))
```

The intended meaning is mathematical: use the smaller resolved length. In the
todo app case, this lets the app card stay content-sized while it is small, but
cap itself to the available fill slot before it would push the info footer out
of the viewport. Once capped, the entries region can use `height(fill())` and
`scrollbar_y()` inside the card.

This plan intentionally changes the public mental model of `min/2` and `max/2`
from pixel constraint wrappers into real length combinators:

- `min(a, b)` resolves both lengths and uses the smaller result.
- `max(a, b)` resolves both lengths and uses the larger result.

## Current Code Facts

- `Emerge.UI.Size.min/2` currently accepts only `px(...)` as its first
  argument and returns `{:minimum, min_px, inner}`.
- `Emerge.UI.Size.max/2` currently accepts only `px(...)` as its first
  argument and returns `{:maximum, max_px, inner}`.
- The current names are opposite of mathematical intuition:
  - `min(px(140), content())` means "at least 140px".
  - `max(px(180), fill())` means "at most 180px".
- Elixir validation and EMRG encoding know only those legacy tuples:
  - `{:minimum, number, length}`
  - `{:maximum, number, length}`
- Native length decoding maps EMRG length variants `4` and `5` to:
  - `Length::Minimum(f64, Box<Length>)`
  - `Length::Maximum(f64, Box<Length>)`
- Native layout resolution currently clamps those wrappers after resolving the
  inner length.
- Row and column fill distribution currently finds a child fill weight by
  recursively looking for `Fill` or `FillWeighted` inside a length wrapper.

## Desired Semantics

The following expressions should be valid:

```elixir
height(min(content(), fill()))
width(min(px(640), fill()))
height(max(px(120), content()))
width(max(content(), px(180)))
```

Meaning:

- `min(content(), fill())`: content-sized until content exceeds the fill slot.
- `min(px(640), fill())`: fill available space, capped at 640px.
- `max(px(120), content())`: content-sized, with a 120px floor.
- `max(content(), px(180))`: same floor semantics with argument order reversed.

The old examples should be rewritten mechanically:

```elixir
# old: at least 140px
width(min(px(140), content()))

# new
width(max(px(140), content()))

# old: at most 180px
width(max(px(180), fill()))

# new
width(min(px(180), fill()))
```

Do not support a mixed model where `min(px(...), length)` keeps the old
"minimum bound" meaning while `min(content(), fill())` means mathematical min.
That would be too easy to misread in real UI code.

## Public API Plan

- Change `Emerge.UI.Size.min/2` to accept two `length()` values and return a new
  tuple shape, for example `{:min, left, right}`.
- Change `Emerge.UI.Size.max/2` to accept two `length()` values and return a new
  tuple shape, for example `{:max, left, right}`.
- Update `length()` types, docs, examples, and doctests to describe
  mathematical combinators.
- Update all repo and demo usages of old `min/2`/`max/2` semantics.

Use mathematical `min/2` and `max/2` everywhere.

## Wire Format And Native Types

Replace the old EMRG length variants instead of carrying legacy decode support.
This is a breaking semantic change, and that is acceptable for this slice.

Recommended native model:

```rust
enum Length {
    Fill,
    Content,
    Px(f64),
    FillWeighted(f64),
    Min(Box<Length>, Box<Length>),
    Max(Box<Length>, Box<Length>),
}
```

Encoding plan:

- Reuse variant `4` for mathematical `Min(left, right)`.
- Reuse variant `5` for mathematical `Max(left, right)`.
- Change the payload shape for variants `4` and `5` from `f64 + length` to
  `length + length`.
- Elixir should encode new public `{:min, left, right}` and
  `{:max, left, right}` as variants `4` and `5`.
- Remove public validation, codec branches, native enum variants, and layout
  branches for legacy `{:minimum, ...}` / `{:maximum, ...}`.

Old binaries encoded with the previous meaning are not expected to be accepted
after this change. Emerge sends fresh trees from Elixir to the native runtime,
so there is no durable on-disk layout payload that needs compatibility.

## Layout Algorithm

### Basic Resolution

Length resolution should become recursive over both branches:

```text
resolve(min(a, b)) = min(resolve(a), resolve(b))
resolve(max(a, b)) = max(resolve(a), resolve(b))
```

Pixel values scale with layout scale exactly as they do today. `content()` and
`fill()` remain semantic lengths, not pixel values.

### Fill Participation

`height(min(content(), fill()))` must participate in parent column fill
distribution, but its final resolved size may be smaller than the allocated fill
slot.

For row/column planning:

- A length requests fill if either branch requests fill.
- A length requests content measurement if either branch requests content.
- The parent should measure the child content first.
- The parent should allocate a fill slot for the child when the expression
  contains fill.
- The child planned size should be the min/max result of content size and fill
  slot.

For the todo layout:

```elixir
column([height(fill())], [
  title_banner(),
  todo_app_base([height(min(content(), fill()))]),
  info_footer()
])
```

The page column treats the app as a fill participant, but the app resolves to
its content height while content is smaller than the remaining viewport slot.
The footer remains immediately below the app because top-aligned column children
are still placed by their final resolved heights.

### Weighted Fill

Be careful with combinators that contain weighted fill. The current row/column
resolver passes a per-child fill allocation into `resolve_length/3`, which is
enough for one fill-bearing branch but not expressive for arbitrary nested
expressions such as `min(fill(2), fill(1))`.

Initial implementation must support fill on both branches. Do not reject nested
or multiple `fill()` leaves in `min/2` and `max/2` expressions.

The key issue is that the current planner collapses a child length into one
fill weight before resolving the length. That is fine for `fill()` and for
simple caps like `min(content(), fill())`, but it is not a complete model for
expressions where both branches contain fill:

```elixir
width(min(fill(2), fill(1)))
height(max(min(content(), fill()), fill(2)))
```

Those expressions need every `fill(n)` leaf to resolve against the same
per-portion unit. For example, if one fill portion is `100px`, then
`min(fill(2), fill(1))` should resolve as `min(200, 100)`, not as one
preselected child weight.

The robust implementation is a recursive row/column fill resolver that:

- detects that the child participates in fill distribution;
- computes the shared per-portion unit from all participating children;
- resolves the full length expression with `fill(n)` leaves mapped to
  `unit * n`;
- uses the final min/max result as the child's planned size.

This still needs a single distribution weight per child so the parent can
compute the shared fill unit. Use a recursive fill-weight function:

```text
fill_weight(px) = none
fill_weight(content) = none
fill_weight(fill(n)) = n
fill_weight(min(a, b)) =
  if both branches contain fill: min(fill_weight(a), fill_weight(b))
  if one branch contains fill: that branch's fill weight
  otherwise: none
fill_weight(max(a, b)) =
  if both branches contain fill: max(fill_weight(a), fill_weight(b))
  if one branch contains fill: that branch's fill weight
  otherwise: none
```

That gives the useful behavior:

- `min(content(), fill())` participates as one fill share, then may resolve
  smaller than the allocated slot when content is short.
- `min(px(640), fill())` participates as one fill share, then caps at `640px`.
- `max(px(120), fill())` participates as one fill share, and can exceed the
  computed fill slot when the minimum size is larger.
- `min(fill(2), fill(1))` participates as one fill share.
- `max(fill(2), fill(1))` participates as two fill shares.

This keeps row/column layout single-pass after measurement, matches the current
behavior where capped fill children can leave unused space, and avoids adding a
constraint solver for this slice.

## Scroll Interaction

The target behavior depends on scroll containers receiving a real bounded
height during resolve.

For the todo app:

- `todo_app_base` uses `height(min(content(), fill()))`.
- `entries()` keeps `height(fill())` and `scrollbar_y()`.
- `input_bar()` and `controls()` keep fixed/content heights.
- When the app card is uncapped, entries resolve to content height.
- When the app card is capped by the fill slot, entries receive the remaining
  bounded height inside the card and become scrollable.

Add native layout tests that prove:

- few entries keep the footer directly below the app card;
- many entries keep the footer visible;
- the entries frame is bounded and scroll content height exceeds viewport
  height in the overflowing case.

## Animation And Validation

- Update `validate_length!/3` in both validation modules for `{:min, left,
  right}` and `{:max, left, right}`.
- Update animation length compatibility so min/max keyframes must keep the same
  combinator structure and compatible branch structures.
- Update native length interpolation to interpolate matching `Min` and `Max`
  branches recursively.
- Update layout-scale length scaling to scale pixel leaves inside both branches.

## Docs And Showcase

- Update `Emerge.UI.Size` docs so `min/2` and `max/2` are mathematical
  combinators.
- Update `ui-size-min-max` example and the layout showcase cards.
- Add a showcase example for `height(min(content(), fill()))` using a bounded
  column with a footer below a scrollable middle region.
- Update any demo code that used old constraint semantics:
  - old `width(min(px(140), shrink()))` becomes
    `width(max(px(140), shrink()))`;
  - old `width(max(px(180), fill()))` becomes
    `width(min(px(180), fill()))`.

## Tests And Verification

Elixir:

- `Emerge.UI.Size.min/2` and `max/2` accept arbitrary valid lengths.
- Invalid nested lengths still fail validation with clear messages.
- Attribute codec round-trips new `{:min, ...}` and `{:max, ...}`.
- Animation validation accepts compatible min/max structures and rejects
  incompatible ones.

Rust:

- Decode reused length variants `4` and `5` as `Min` and `Max`.
- Resolve `Min` and `Max` over px/content/fill combinations.
- Row and column planning handles `min(content, fill)` as a bounded fill child.
- Row and column planning resolves multiple fill leaves recursively, including
  `min(fill(2), fill(1))` and `max(fill(2), fill(1))`.
- Scroll containers inside a `min(content, fill)` parent receive bounded height
  once the parent is capped.
- Layout scale scales pixel leaves inside `Min` and `Max`.
- Existing `Minimum` and `Maximum` tests are migrated to mathematical `Min` and
  `Max` coverage, with old semantic assertions rewritten.

Validation commands:

```bash
cargo test --manifest-path native/emerge_skia/Cargo.toml
mix test
cd ../emerge_demo && mix test
```

Run focused layout benchmarks before and after if the row/column fill planner
needs more than a local resolver change.

## Open Questions

None currently. The implementation should use the recursive fill resolver from
the first slice.

## Implementation Notes

- `Emerge.UI.Size.min/2` and `max/2` now produce `{:min, left, right}` and
  `{:max, left, right}`.
- EMRG length variants `4` and `5` now encode/decode `length + length` payloads
  for mathematical min/max.
- Native layout uses a recursive planned-length resolver so each `fill(n)` leaf
  resolves against the shared row/column fill unit.
- Content expansion now respects min caps, so `height(min(content(), fill()))`
  can reserve a bounded slot and let internal scroll regions take over.
