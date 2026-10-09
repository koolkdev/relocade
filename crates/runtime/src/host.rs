//! Memory mutations and code validity share the Store's execution-thread owner.

use wasm86_code_cache::{CodeCache, CodeRange, Mapping, Ticket};
use wasm86_codegen::{Entry, Profile, Request};
use wasm86_x86::CpuState;
use wasmtime::{AsContext, AsContextMut, Caller, Linker, Memory};

/// Construct the Store with this wrapper before allocating memories/instances.
/// Host callbacks retain ordinary access to their payload; coherence needs no lock.
pub struct HostState<T> {
    pub host: T,
    code: Option<CodeCache>,
}
impl<T> HostState<T> {
    pub fn new(host: T) -> Self {
        Self { host, code: None }
    }
    fn code(&self) -> &CodeCache {
        self.code.as_ref().expect("memory owner is attached")
    }
    fn code_mut(&mut self) -> &mut CodeCache {
        self.code.as_mut().expect("memory owner is attached")
    }
}

/// Handles for one private guest memory set. Mutations work through a Store or
/// Caller, so devices use the same invalidation ordering as external host writes.
#[derive(Clone, Copy)]
pub struct HostMemory {
    cpu: Memory,
    guest: Memory,
    mapping: Memory,
    profile: Profile,
}
impl HostMemory {
    /// Adopts initialized memory and its mapping table. A Store has one coherence
    /// owner; raw guest/table writes after adoption must obey its protocol.
    pub fn new<T>(
        mut store: impl AsContextMut<Data = HostState<T>>,
        cpu: Memory,
        guest: Memory,
        mapping: Memory,
        profile: Profile,
    ) -> Self {
        let mut store = store.as_context_mut();
        assert!(store.data().code.is_none(), "one coherence owner per Store");
        let code = CodeCache::new(profile.into(), mapping.data(&store));
        store.data_mut().code = Some(code);
        Self {
            cpu,
            guest,
            mapping,
            profile,
        }
    }

    pub fn profile(self) -> Profile {
        self.profile
    }
    pub fn cpu_memory(self) -> Memory {
        self.cpu
    }
    pub fn guest_memory(self) -> Memory {
        self.guest
    }
    pub fn mapping_memory(self) -> Memory {
        self.mapping
    }

    pub fn read_cpu<T>(self, store: impl AsContext<Data = HostState<T>>) -> CpuState {
        CpuState::from_bytes(
            self.cpu.data(store.as_context())[..CpuState::BYTE_LEN]
                .try_into()
                .unwrap(),
        )
    }

    /// CPU edits are host-boundary operations. Entry selection checks the loaded
    /// profile and CS before using any compiled code.
    pub fn write_cpu<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        cpu: &CpuState,
    ) -> wasmtime::Result<()> {
        self.cpu.write(store.as_context_mut(), 0, &cpu.to_bytes())?;
        Ok(())
    }

    /// Invalidates all code aliases before changing backing bytes, including DMA.
    pub fn write_backing<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        offset: u32,
        bytes: &[u8],
    ) -> wasmtime::Result<()> {
        let mut store = store.as_context_mut();
        if !(offset as usize)
            .checked_add(bytes.len())
            .is_some_and(|end| end <= self.guest.data_size(&store))
        {
            return Err(wasmtime::Error::msg("backing write outside memory"));
        }
        let length = u32::try_from(bytes.len())?;
        let (table, data) = self.mapping.data_and_store_mut(&mut store);
        data.code_mut().invalidate_backing(table, offset, length);
        self.guest.write(&mut store, offset as usize, bytes)?;
        Ok(())
    }

    pub fn remap<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        page: u32,
        mapping: Mapping,
    ) {
        let (table, data) = self.mapping.data_and_store_mut(store.as_context_mut());
        data.code_mut().remap(table, page, mapping);
    }

    pub fn invalidate_all<T>(self, mut store: impl AsContextMut<Data = HostState<T>>) {
        let (table, data) = self.mapping.data_and_store_mut(store.as_context_mut());
        data.code_mut().clear(table);
    }

    pub(crate) fn enter<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
    ) -> Option<(u32, Option<Ticket>)> {
        let mut store = store.as_context_mut();
        let cpu = self.read_cpu(&store);
        let (table, data) = self.mapping.data_and_store_mut(&mut store);
        let code = data.code_mut();
        code.enter(&cpu, table)
            .then(|| (cpu.eip, code.lookup(cpu.eip)))
    }

    pub(crate) fn capture<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        eip: u32,
        instruction_limit: u32,
    ) -> Option<(Ticket, Request)> {
        let mut store = store.as_context_mut();
        let cpu = self.read_cpu(&store);
        let (table, data) = self.mapping.data_and_store_mut(&mut store);
        let capture = data
            .code_mut()
            .capture(&cpu, eip, instruction_limit, table)?;
        // No host/guest execution or asynchronous wait occurs between watch and copy.
        let code = capture.copy_bytes(self.guest.data(&store));
        Some((
            capture.ticket,
            Request {
                profile: self.profile,
                entry: Entry::Block {
                    eip,
                    code,
                    instruction_limit,
                },
            },
        ))
    }

    pub(crate) fn register<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        eip: u32,
        ranges: &[CodeRange],
    ) -> Option<Ticket> {
        let mut store = store.as_context_mut();
        let cpu = self.read_cpu(&store);
        let (table, data) = self.mapping.data_and_store_mut(&mut store);
        data.code_mut().register(&cpu, eip, ranges, table)
    }

    pub(crate) fn is_pending<T>(
        self,
        store: impl AsContext<Data = HostState<T>>,
        ticket: Ticket,
    ) -> bool {
        store.as_context().data().code().is_pending(ticket)
    }

    pub(crate) fn contains<T>(
        self,
        store: impl AsContext<Data = HostState<T>>,
        ticket: Ticket,
    ) -> bool {
        store.as_context().data().code().contains(ticket)
    }

    pub(crate) fn install<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        ticket: Ticket,
    ) -> bool {
        let (table, data) = self.mapping.data_and_store_mut(store.as_context_mut());
        data.code_mut().install(ticket, table)
    }

    pub(crate) fn cancel<T>(
        self,
        mut store: impl AsContextMut<Data = HostState<T>>,
        ticket: Ticket,
    ) {
        let (table, data) = self.mapping.data_and_store_mut(store.as_context_mut());
        data.code_mut().cancel(ticket, table);
    }

    pub(crate) fn detach<T>(self, mut store: impl AsContextMut<Data = HostState<T>>) {
        self.invalidate_all(store.as_context_mut());
        store.as_context_mut().data_mut().code = None;
    }

    pub(crate) fn define<T: 'static>(
        self,
        store: impl AsContext<Data = HostState<T>>,
        linker: &mut Linker<HostState<T>>,
    ) -> wasmtime::Result<()> {
        let table_name = if self.profile == Profile::Real16 {
            "physicalMap"
        } else {
            "machine"
        };
        for (name, memory) in [
            ("cpuState", self.cpu),
            ("guest", self.guest),
            (table_name, self.mapping),
        ] {
            linker.define(store.as_context(), "wasm86", name, memory)?;
        }
        linker.func_wrap(
            "wasm86",
            "invalidateCode",
            move |mut caller: Caller<'_, HostState<T>>, address: u32, bytes: u32| {
                let (table, data) = self.mapping.data_and_store_mut(&mut caller);
                data.code_mut().invalidate_write(table, address, bytes);
            },
        )?;
        Ok(())
    }
}
