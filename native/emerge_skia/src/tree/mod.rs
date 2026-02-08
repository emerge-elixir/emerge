//! Tree structures for Emerge element trees.
//!
//! This module provides:
//! - `Element` and related types for representing the UI tree
//! - `Attrs` for decoded element attributes
//! - Deserialization from the EMRG binary format
//! - Patch application for incremental updates
//! - Layout engine for computing element frames

#![allow(unused_imports)] // Re-exports are part of the public API even if unused internally.

pub mod attrs;
pub mod deserialize;
pub mod element;
pub mod layout;
pub mod patch;
pub mod render;
pub mod scrollbar;
pub mod serialize;
