//! `cargo ularyt`: swap unmaintained dependencies for their maintained
//! successors, in the manifest and in the source.
//!
//! The catalogue in [`rules`] decides which swaps exist. A swap is automatic
//! only when it is mechanical: the successor exposes the same items under a new
//! name, or std offers a direct equivalent. Everything else is reported with
//! the recommended successor and left alone. When a single use in a package
//! cannot be translated with certainty, the whole swap for that package is
//! blocked and nothing is changed, so the dependency never ends up half-moved.

#![forbid(unsafe_code)]

pub mod apply;
pub mod lexer;
pub mod plan;
pub mod project;
pub mod report;
pub mod rewrite;
pub mod rules;
