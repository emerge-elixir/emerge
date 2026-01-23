//! Tree structures for Emerge element trees.
//!
//! This module provides:
//! - `Element` and related types for representing the UI tree
//! - `Attrs` for decoded element attributes
//! - Deserialization from the EMRG binary format
//! - Patch application for incremental updates
//! - Layout engine for computing element frames

mod attrs;
mod element;
mod deserialize;
mod layout;
mod patch;
mod render;

pub use attrs::{
    Attrs, Length, Padding, AlignX, AlignY, Color, Background, Font, FontWeight, FontStyle,
    decode_attrs,
};
pub use element::{Element, ElementId, ElementKind, ElementTree, Frame};
pub use deserialize::decode_tree;
pub use layout::{Constraint, layout_tree_default};
pub use patch::{Patch, apply_patches, decode_patches};
pub use render::render_tree;
