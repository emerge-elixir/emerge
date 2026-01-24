# EmergeSkia

Rust-backed rendering with an Elixir tree API and EMRG encoding/patching.

## Docs
- Tree identity, reconciliation, and patching: `TREE_PATCHING.md`
- Binary encoding format: `EMRG_FORMAT.md`

## Elixir API (tree + EMRG)
```elixir
state = Emerge.diff_state_new()
tree = Emerge.UI.column([id: :root], [Emerge.UI.el(Emerge.UI.text("Hello"))])

{full_bin, state, _assigned} = Emerge.encode_full(state, tree)
{patch_bin, state, _assigned} = Emerge.diff_state_update(state, tree)
```

## Spacing & Distribution

Use `spacing/1` for uniform gaps, `spacing_xy/2` to control horizontal vs vertical
gaps, and `space_evenly/0` to distribute children with equal gaps between them
(elm-ui "space-evenly" maps to `space-between`).

```elixir
row([width(fill()), space_evenly()], [
  chip("One"),
  chip("Two"),
  chip("Three")
])

wrapped_row([width(fill()), spacing_xy(6, 14)], [
  chip("Spacing"),
  chip("X/Y"),
  chip("Example"),
  chip("Wrapped")
])
```

## Installation

If [available in Hex](https://hex.pm/docs/publish), the package can be installed
by adding `emerge_skia` to your list of dependencies in `mix.exs`:

```elixir
def deps do
  [
    {:emerge_skia, "~> 0.1.0"}
  ]
end
```

Documentation can be generated with [ExDoc](https://github.com/elixir-lang/ex_doc)
and published on [HexDocs](https://hexdocs.pm). Once published, the docs can
be found at <https://hexdocs.pm/emerge_skia>.
