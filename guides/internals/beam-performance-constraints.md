# BEAM Performance Constraints

This document captures the BEAM-specific constraints that should shape Elixir-side
reconciliation, diffing, serialization, and event-registry work in Emerge.

The goal is not to chase folklore or micro-optimizations blindly. The goal is to
make design choices that fit the actual runtime model of Erlang/Elixir so that
future implementation work starts from the right constraints.

## Scope

Apply this guidance to:

- `lib/emerge/engine/reconcile.ex`
- `lib/emerge/engine/diff_state.ex`
- `lib/emerge/engine/patch.ex`
- `lib/emerge/engine/serialization.ex`
- any future Elixir-side identity, diffing, patching, or registry data structures

## Core Rules

1. Treat lists as linked lists.
2. Build lists by prepending and reverse once at the end.
3. Use maps for lookup-heavy structures.
4. Keep ordered collections and lookup indexes separate.
5. Prefer fixed-width numeric ids once ids are numeric.
6. Prefer incremental derived-data maintenance over full-tree rebuilds.
7. Measure before introducing less idiomatic BEAM structures.

## Lists

Lists are excellent for:

- linear traversal
- recursive descent
- preserving order
- prepend-based accumulation

Lists are a bad fit for:

- random access
- repeated positional lookup
- repeated appends using `++`

### Do

- use `[item | acc]` in hot accumulation paths
- use `Enum.reverse/1` once at the end
- use `Enum.reverse(list, tail)` when stitching a forward list onto a reversed accumulator
- write unkeyed sibling reconciliation as a linear walk, not repeated indexed lookup

### Avoid

- `Enum.at(list, i)` inside loops over sibling lists
- repeated `Enum.drop/2` when a single forward traversal would do
- repeated `++` inside reducers
- `lists:flatten`-style thinking when iodata or a deep list is acceptable

### Why this matters for Emerge

Current reconciliation already shows the patterns we should avoid:

- indexed sibling lookups with `Enum.at/2`
- repeated patch accumulation with `++`

At Emerge's expected sizes, those patterns are much more important than small
syntax choices such as body recursion versus tail recursion.

## Maps

Maps are the default good lookup structure on the BEAM.

They are a good fit for:

- `DiffState`
- `VNode`
- global keyed indexes
- `id -> meta` indexes
- event registries

Small fixed-shape maps and structs are fine. Do not try to remove them unless a
measured hotspot proves otherwise.

### Do

- use maps/structs for fixed-shape state and dictionaries
- update multiple known keys together when it makes the code clearer
- prefer direct map syntax when working with fixed keys

### Avoid

- using maps as ordered collections
- rebuilding derived maps repeatedly when incremental maintenance is practical

## Arrays And Other Dense Structures

Do not assume that a numeric key implies that `:array` or tuple-indexed storage
is the right answer.

That only tends to pay off when keys are:

- dense
- stable in density over time
- used in a proven hot path

For the current Emerge direction:

- Elixir-side `NodeId` will likely be monotonic and non-reused
- that means ids become sparse over long sessions
- sparse monotonic ids are usually a better fit for maps than arrays on the Elixir side

Dense indexed storage is a much better fit for the Rust-side `NodeIx` model.

### Do

- default to maps for `NodeId`-indexed Elixir structures
- require a measured reason before introducing `:array`, ETS, or tuple-indexed storage

## Ordered Collections Versus Lookup Indexes

One structure should not be forced to do two incompatible jobs.

For Elixir-side tree and diff structures:

- use lists for ordered children and nearby mounts
- use maps for keyed lookup, node-id lookup, and other dictionary operations

This is especially important for reconciliation.

Recommended shape:

- ordered children: list
- ordered nearby mounts: list
- global key index: map
- `id -> vnode/meta`: map

This supports both:

- cheap order-preserving rebuilds
- cheap keyed lookup

without pretending linked lists are random-access arrays.

## Binaries And Iodata

For patch encoding and wire serialization:

- prefer fixed-width binary fields once ids are numeric
- prefer iodata/deep-list construction where possible
- avoid flattening intermediate data eagerly

### Do

- encode numeric ids as fixed-width binary fields such as `u64`
- assemble patch payloads as iodata when it stays readable

### Avoid

- repeated `term_to_binary`/`binary_to_term` on numeric ids once the new id model exists
- flattening before it is required

### Why this matters for Emerge

The current code repeatedly term-encodes ids in:

- patch encoding
- tree serialization
- event registry keys

Once ids are numeric, fixed-width encoding is both simpler and faster.

## Derived Data

A full-tree rebuild of derived data is acceptable as a temporary baseline, not as
the target design if subtree-local updates are realistic.

Examples of derived data:

- event registry
- global key index
- node-id index
- patch-order or mount-order helper structures

### Do

- ask whether a structure really needs full-tree rebuilds
- prefer subtree-local or incremental maintenance when the design already has stable identity

### Avoid

- forcing a global traversal on every small subtree change when incremental updates are straightforward

### Why this matters for Emerge

The current event registry is rebuilt from the full assigned tree after each diff.
That is likely to matter more than many smaller syntactic optimizations.

## Tail Recursion Versus Folklore

Do not cargo-cult tail recursion as the primary optimization tool.

For list-producing code on modern BEAM runtimes, the larger questions are usually:

- are we constructing lists efficiently?
- are we doing too many passes?
- are we forcing random-access behavior on linked lists?
- are we allocating too many intermediate collections?

Focus on those first.

## Process Boundaries

When spawning or sending closures between processes:

- extract only what is needed
- avoid accidentally copying large trees, maps, or shared subterms

This matters less for the reconciliation redesign itself, but it is still part of
the BEAM cost model and should be kept in mind.

## Design Checklist

Apply this checklist to every upcoming Elixir-side design decision.

1. Does this require random access on a list?
2. Does this append with `++` in a loop or reducer?
3. Could this be implemented in one pass instead of several?
4. Should this be represented as a map index plus a list order rather than one mixed structure?
5. Is this forcing a full-tree rebuild of derived data that could be subtree-local?
6. Is this encoding numeric ids inefficiently?
7. Is this introducing a less idiomatic structure without a measured need?

If any answer is "yes", the design should be questioned before implementation.

## Implications For The Current Planning Work

These constraints strongly support the current planning direction:

- shared `NodeId` plus native `NodeIx`
- Elixir lists for ordered children and nearby mounts
- Elixir maps for global key indexes and node-id indexes
- prepend-and-reverse reconciliation
- fixed-width `u64` id encoding in EMRG
- avoiding `Enum.at/2`-driven reconciliation logic
- eventually moving full-tree derived-data rebuilds toward incremental maintenance

That is the shape most aligned with BEAM efficiency while still keeping the
design simple and understandable.
