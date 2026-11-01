mod address;
mod declaring;
mod identity;
mod resolution;
mod selection;

pub use declaring::{Declarable, declarable};
pub use identity::Identity;
pub use resolution::{Purpose, Resolved, resolved};
pub use selection::{Bound, Declaring, Selecting, Selection, bound, resolution};

#[cfg(test)]
mod tests;
