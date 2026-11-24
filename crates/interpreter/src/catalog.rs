// Shared by the build script and the packaged lookup so every listed input
// selects the file generated with those same guarantees.
macro_rules! interpreter_catalog {
    ($file:ident) => {
        [
            (
                EntryAssumptions::LIVE,
                MemoryBackend::PhysicalBus,
                $file!("pc.wasm"),
            ),
            (
                EntryAssumptions::FLAT32,
                MemoryBackend::HostPageTable,
                $file!("flat32.wasm"),
            ),
            (
                EntryAssumptions::SEGMENTED32,
                MemoryBackend::HostPageTable,
                $file!("segmented32.wasm"),
            ),
            (
                EntryAssumptions::SEGMENTED16,
                MemoryBackend::HostPageTable,
                $file!("segmented16.wasm"),
            ),
        ]
    };
}

pub(crate) use interpreter_catalog;
