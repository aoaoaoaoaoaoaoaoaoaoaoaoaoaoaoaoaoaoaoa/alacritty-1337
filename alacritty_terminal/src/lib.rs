//! Alacritty - The GPU Enhanced Terminal.

#![cfg_attr(
    test,
    allow(
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::unwrap_used,
        unused_crate_dependencies,
        unused_results,
        reason = "tests use deliberate failure shortcuts and discard fixture mutations"
    )
)]

pub mod event;
pub mod event_loop;
pub mod grid;
pub mod index;
pub mod selection;
pub mod sync;
pub mod term;
pub mod thread;
pub mod tty;
pub mod vi_mode;

pub use crate::grid::Grid;
pub use crate::term::Term;
pub use vte;
