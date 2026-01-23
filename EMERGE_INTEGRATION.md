# Emerge Integration Plan

This document outlines the plan to integrate Emerge's layout engine into emerge_skia, moving layout computation and rendering to Rust with direct Skia rendering.

## Architecture Overview

```
Elixir (emerge)                         Rust (emerge_skia)
─────────────────                       ──────────────────
Element tree (DSL)
    │
    ▼
Reconcile (assign IDs)
    │
    ▼
Serialize (EMRG binary)  ──────────►   Deserialize
    │                                       │
    ▼                                       ▼
Diff/Patch binary        ──────────►   Apply patches
                                            │
                                            ▼
                                       Layout engine
                                       (measure → resolve)
                                            │
                                            ▼
                                       Skia rendering
                                            │
                                            ▼
                                       Window/Display
```

## Implementation Phases

### Phase 1: Deserialization & Tree Structure ◄ CURRENT

1. Define Rust `Element` struct mirroring Elixir's structure
2. Deserialize the "EMRG" binary format (full tree)
3. Build in-memory tree with ID-based lookup

**EMRG Binary Format:**
- Header: `"EMRG"` (4 bytes) + version (1 byte) + node count (4 bytes)
- Per node:
  - id: length-prefixed Erlang term binary
  - type tag: 1 byte (row=1, column=2, el=3, text=4, none=5)
  - attrs: length-prefixed Erlang term binary
  - children: count (4 bytes) + list of length-prefixed id term binaries

### Phase 2: Patch Application

1. Decode patch operations from binary stream
2. Implement patch types:
   - `set_attrs` (tag=1): Update node attributes
   - `set_children` (tag=2): Reorder children
   - `insert_subtree` (tag=3): Insert new subtree
   - `remove` (tag=4): Remove node
3. Maintain HashMap<Id, Node> for O(1) lookup

### Phase 3: Layout Engine

Port the 3-pass layout algorithm from Elixir:

1. **Measurement (bottom-up)**
   - Compute intrinsic sizes for each element
   - Text: use Skia font metrics
   - Containers: aggregate children sizes + padding/spacing

2. **Resolution (top-down)**
   - Apply constraints to resolve final sizes
   - Handle Length types: `:content`, `:fill`, `{:px, n}`, `{:fill, portion}`
   - Compute frames: `{x, y, width, height}`

3. **Output**
   - Tree with all frames populated
   - Ready for rendering

### Phase 4: Skia Rendering

1. Traverse laid-out tree
2. Map elements to Skia draw commands:
   - `el` → rounded rect with background/border
   - `text` → Skia text rendering
   - `row`/`column` → group with clip if scrollable
3. Integrate with existing emerge_skia renderer

## Data Structures

### Rust Element
```rust
pub struct Element {
    pub id: ElementId,
    pub kind: ElementKind,
    pub attrs: Attrs,
    pub children: Vec<ElementId>,
    pub frame: Option<Frame>,
}

pub enum ElementKind {
    Row,
    Column,
    El,
    Text(String),
    None,
}

pub struct Frame {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub struct Attrs {
    pub width: Length,
    pub height: Length,
    pub padding: Padding,
    pub spacing: f32,
    pub align_x: Alignment,
    pub align_y: Alignment,
    pub background: Option<Color>,
    pub border: Option<Border>,
    // ... etc
}
```

### Tree Storage
```rust
pub struct ElementTree {
    pub root: Option<ElementId>,
    pub nodes: HashMap<ElementId, Element>,
}
```

## NIF Interface

```rust
// Full tree upload (initial render)
fn upload_tree(binary: Binary) -> Result<(), String>;

// Apply patches (incremental updates)
fn apply_patches(binary: Binary) -> Result<(), String>;

// Trigger layout + render
fn layout_and_render(width: u32, height: u32) -> Result<(), String>;
```

## File Structure

```
native/emerge_skia/src/
├── lib.rs              # NIF entry
├── renderer.rs         # DrawCmd, SkiaRenderer (existing)
├── input.rs            # Input handling (existing)
├── backend/            # Display backends (existing)
│   ├── wayland.rs
│   ├── raster.rs
│   └── drm.rs (future)
├── tree/               # NEW: Tree structures
│   ├── mod.rs
│   ├── element.rs      # Element, ElementKind, Attrs
│   ├── deserialize.rs  # EMRG format parsing
│   └── patch.rs        # Patch application
└── layout/             # NEW: Layout engine
    ├── mod.rs
    ├── measure.rs      # Bottom-up measurement
    └── resolve.rs      # Top-down constraint resolution
```

## Reference

- Elixir serialization: `/workspace/emerge/lib/emerge/serialization.ex`
- Elixir patch encoding: `/workspace/emerge/lib/emerge/patch.ex`
- Elixir layout: `/workspace/emerge/lib/emerge/layout.ex`
- Tree patching docs: `/workspace/emerge/TREE_PATCHING.md`
