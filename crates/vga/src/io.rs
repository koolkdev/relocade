//! Retained port and memory bindings for one connected card.
use crate::{mmio::MmioWindow, ports::Ports};

/// Used when decoding changes. Ordinary accesses invoke the installed operations.
pub(crate) struct Io<Card> {
    pub ports: Ports,
    pub memory_windows: Vec<MmioWindow<Card>>,
}
