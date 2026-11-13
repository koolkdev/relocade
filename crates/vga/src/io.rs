//! Retained port and memory bindings for one connected card.
use crate::{mmio::MemoryMapping, ports::Ports};

/// Used when decoding changes. Ordinary accesses invoke the installed operations.
pub(crate) struct Io {
    pub ports: Ports,
    pub memory: MemoryMapping,
}
