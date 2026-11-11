//! One port map for synchronous host routing and the CPU's Wasm tables.

mod handlers;
mod map;

pub(crate) use handlers::{PortRead, PortWrite};
pub(crate) use map::Ports;
