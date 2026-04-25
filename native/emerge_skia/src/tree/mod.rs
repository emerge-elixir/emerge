//! Tree structures for Emerge element trees.
//!
//! This module provides:
//! - `Element` and related types for representing the UI tree
//! - `Attrs` for decoded element attributes
//! - Deserialization from the EMRG binary format
//! - Patch application for incremental updates
//! - Layout engine for computing element frames

pub mod animation;
pub mod attrs;
pub mod deserialize;
pub mod element;
pub mod geometry;
pub mod invalidation;
pub mod layout;
pub mod patch;
pub mod render;
pub mod scene;
pub mod scrollbar;
pub mod serialize;
pub mod text_layout;
pub mod transform;
