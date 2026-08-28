//! Game subsystem: process supervision and launch argument construction.
pub mod instance;
pub mod pipeline;
pub mod process;
pub use process::{launch, Session};
#[allow(unused_imports)]
pub use process::{Level, LogLine};
