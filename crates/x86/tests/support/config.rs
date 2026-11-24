//! Explicit compiler inputs for the four instruction-test configurations.

use relocade_x86::{CompilerConfig, EntryAssumptions, MemoryBackend};

pub(crate) const REAL16: CompilerConfig =
    CompilerConfig::new(EntryAssumptions::REAL_MODE, MemoryBackend::PhysicalBus);
pub(crate) const FLAT32: CompilerConfig =
    CompilerConfig::new(EntryAssumptions::FLAT32, MemoryBackend::HostPageTable);
pub(crate) const SEGMENTED16: CompilerConfig =
    CompilerConfig::new(EntryAssumptions::SEGMENTED16, MemoryBackend::HostPageTable);
pub(crate) const SEGMENTED32: CompilerConfig =
    CompilerConfig::new(EntryAssumptions::SEGMENTED32, MemoryBackend::HostPageTable);

/// Exercises protected execution on the PC bus with IF and IOPL read from the CPU.
pub(crate) const fn physical_with_live_flags(entry: EntryAssumptions) -> CompilerConfig {
    CompilerConfig::new(
        EntryAssumptions {
            interrupt_flag: None,
            iopl: None,
            ..entry
        },
        MemoryBackend::PhysicalBus,
    )
}
