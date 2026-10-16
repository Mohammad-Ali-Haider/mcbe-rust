//! Named dimension definitions, compiled before use; visited worlds keep snapshots.
mod definitions;
pub use definitions::{Definition, Definitions};

mod state;
pub use state::{Saved, State, Travel, attach_legacy};

pub mod screen;
