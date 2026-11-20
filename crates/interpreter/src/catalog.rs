// The build writes these files and the runtime embeds their Wasm.
macro_rules! interpreter_catalog {
    ($file:ident) => {
        [
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
            (
                EntryAssumptions::REAL_MODE,
                MemoryBackend::PhysicalBus,
                $file!("real16.wasm"),
            ),
        ]
    };
}

pub(crate) use interpreter_catalog;
