//! A Flat32 x86 embedding with one mapped 64 KiB guest region. Each guest
//! branch returns to the test dispatcher at the published destination.
use relocade_test_support::{engine, SharedBytes};
use relocade_wasm_bindings::{lift_flat, lower_flat, Flat};
use relocade_win32::{Exception, GuestMemory};
use relocade_x86::{
    compile_block_from_bytes, page_table, CpuState, ExecutionContext, ExecutionExit, HostExit,
    SegmentProfile,
};
use wasmtime::{Linker, Memory, MemoryType, Module, Ref, RefType, Store, Table, TableType};

pub const BASE: u32 = 0x7050_0000;
const BYTES: u32 = 65536;

#[derive(Clone, Copy)]
pub enum Engine {
    Block,
    Interpreter,
}

pub struct Flat32 {
    ram: SharedBytes,
    state: SharedBytes,
    store: Store<()>,
    linker: Linker<()>,
}

impl Flat32 {
    pub fn new() -> Self {
        let ram = SharedBytes::new(1, 1);
        let state = SharedBytes::new(1, 1);
        state.write(0, &ExecutionContext::default().to_bytes());
        let mut store = Store::new(engine(), ());
        let pages = Memory::new(&mut store, MemoryType::new(64, None)).unwrap();
        for frame in (0..BYTES).step_by(page_table::PAGE_BYTES as usize) {
            pages
                .write(
                    &mut store,
                    page_table::entry_offset(BASE + frame) as usize,
                    &page_table::encode_entry(frame, true).unwrap(),
                )
                .unwrap();
        }
        let mut linker = Linker::new(engine());
        linker
            .define(
                &store,
                "relocade",
                "executionContext",
                state.memory().clone(),
            )
            .unwrap();
        linker
            .define(&store, "relocade", "guest", ram.memory().clone())
            .unwrap();
        linker.define(&store, "relocade", "machine", pages).unwrap();
        linker
            .func_wrap("relocade", "dispatch", || {
                lower_flat::<ExecutionExit>(ExecutionExit::Host(HostExit::Return))
            })
            .unwrap();
        linker
            .func_wrap("relocade", "resolveCall", || (1u32, 1u32))
            .unwrap();
        let dispatch = linker
            .get(&mut store, "relocade", "dispatch")
            .unwrap()
            .into_func()
            .unwrap();
        let functions = Table::new(
            &mut store,
            TableType::new(RefType::FUNCREF, 2, None),
            Ref::Func(Some(dispatch)),
        )
        .unwrap();
        linker
            .define(&store, "relocade", "executionFunctions", functions)
            .unwrap();
        Self {
            ram,
            state,
            store,
            linker,
        }
    }

    pub fn run(&mut self, cpu: &mut CpuState, code_len: usize, backend: Engine) {
        let (module, entry) = match backend {
            Engine::Block => {
                let mut bytes = vec![0; code_len];
                self.read(cpu.eip, &mut bytes).unwrap();
                let block = compile_block_from_bytes(cpu.eip, &bytes, 32).unwrap();
                (Module::new(engine(), block.bytes).unwrap(), block.entry)
            }
            Engine::Interpreter => {
                let interpreter = relocade_interpreter::module(SegmentProfile::Flat32.into());
                (Module::new(engine(), interpreter).unwrap(), "run".into())
            }
        };
        self.state.write(0, &cpu.to_bytes());
        self.linker
            .define_unknown_imports_as_traps(&module)
            .unwrap();
        let instance = self.linker.instantiate(&mut self.store, &module).unwrap();
        let run = instance
            .get_typed_func::<(), Flat<ExecutionExit>>(&mut self.store, &entry)
            .unwrap();
        let exit = lift_flat::<ExecutionExit>(run.call(&mut self.store, ()).unwrap());
        assert_eq!(exit, Some(ExecutionExit::Host(HostExit::Return)));
        *cpu = CpuState::from_bytes(self.state.read(0, CpuState::BYTE_LEN).try_into().unwrap());
    }
}

fn backing(address: u32, len: usize, write: bool) -> Result<u32, Exception> {
    let offset = address.wrapping_sub(BASE);
    if u64::from(offset) + len as u64 <= u64::from(BYTES) {
        Ok(offset)
    } else {
        Err(Exception::PageFault {
            linear_address: address,
            error_code: if write { 6 } else { 4 },
        })
    }
}

impl GuestMemory for Flat32 {
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Exception> {
        bytes.copy_from_slice(
            &self
                .ram
                .read(backing(address, bytes.len(), false)?, bytes.len()),
        );
        Ok(())
    }
    fn write(&self, address: u32, bytes: &[u8]) -> Result<(), Exception> {
        self.ram.write(backing(address, bytes.len(), true)?, bytes);
        Ok(())
    }
}
