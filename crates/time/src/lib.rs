//! Integer clock timestamps, tick rates and periodic scheduling.
//!
//! A timestamp belongs to one clock. Reading and comparing timestamps requires
//! no duration conversion. Schedules retain their own phase and expose only
//! integer deadlines. These values never read a live clock or register a wait.
#![forbid(unsafe_code)]

mod schedule;
mod tick_rate;
mod timestamp;

pub use schedule::Schedule;
pub use tick_rate::TickRate;
pub use timestamp::Timestamp;
