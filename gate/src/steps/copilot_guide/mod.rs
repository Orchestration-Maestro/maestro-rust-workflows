//! `rust-gate guide` and `guide --check`: a repository's Copilot guide,
//! `.github/copilot-instructions.md`, written from its tracked files and
//! keeping every explanation already given. `text.rs` holds the prose
//! helpers, `describe.rs` explains a file, `tree.rs` draws the annotated
//! tree, `render.rs` writes the guide and `step.rs` holds the steps.

mod describe;
mod render;
mod step;
mod text;
mod tree;

#[cfg(test)]
mod tree_cases;

pub(super) use step::STEPS;
