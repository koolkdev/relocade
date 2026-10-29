//! Application execution and communication on one worker, independent of a window.
#![forbid(unsafe_code)]
#![allow(async_fn_in_trait)]

mod application;
mod task;
mod worker;

pub use application::{Application, Context};
pub use task::{spawn, Task};
