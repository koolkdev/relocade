#![allow(dead_code)]
use relocade_machine::{CpuConfig, CpuStorage, MachineConfig, MemoryStorage};
use relocade_x86::ExecutionContext;
use std::{cell::RefCell, mem::offset_of, rc::Rc};

pub struct Storage {
    pub context: Rc<RefCell<[u8; ExecutionContext::BYTE_LEN]>>,
    pub cpu_reads: RefCell<Vec<(u32, usize)>>,
    bytes: Vec<u8>,
}

impl CpuStorage for Storage {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.cpu_reads.borrow_mut().push((offset, bytes.len()));
        let offset = offset_of!(ExecutionContext, cpu) + offset as usize;
        bytes.copy_from_slice(&self.context.borrow()[offset..][..bytes.len()]);
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        let offset = offset_of!(ExecutionContext, cpu) + offset as usize;
        self.context.borrow_mut()[offset..][..bytes.len()].copy_from_slice(bytes);
    }
}

impl MemoryStorage for Storage {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.bytes[offset as usize..][..bytes.len()]);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.bytes[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
}

impl Storage {
    pub fn new(cpu: &CpuConfig, config: &MachineConfig) -> Self {
        Self {
            context: Rc::new(RefCell::new(
                ExecutionContext {
                    cpu: *cpu.cpu(),
                    instruction_target: 998,
                    cpu_model: cpu.execution_target().cpu_model,
                }
                .to_bytes(),
            )),
            cpu_reads: RefCell::new(Vec::new()),
            bytes: vec![0; config.memory_size() as usize],
        }
    }
}

pub fn initializing_module(offset: u32, bytes: &[u8]) -> Vec<u8> {
    use wasm_encoder::{ConstExpr, DataSection, ImportSection, MemoryType, Module};
    let mut module = Module::new();
    let mut imports = ImportSection::new();
    imports.import(
        "relocade",
        "guest",
        MemoryType {
            minimum: 1,
            maximum: Some(65536),
            memory64: false,
            shared: true,
            page_size_log2: None,
        },
    );
    module.section(&imports);
    let mut data = DataSection::new();
    data.active(
        0,
        &ConstExpr::i32_const(offset as i32),
        bytes.iter().copied(),
    );
    module.section(&data);
    module.finish()
}

/// Initial state for a test's machine and its first CPU.
pub struct Configuration {
    pub cpu: CpuConfig,
    pub machine: MachineConfig,
}

pub fn configuration(
    target: relocade_x86::ExecutionTarget,
    cpu: relocade_x86::CpuState,
    size: u64,
    map: relocade_x86::PhysicalMemoryMap,
) -> Result<Configuration, relocade_machine::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::new(target, cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}

pub async fn create_cpu<
    T: 'static,
    B: relocade_machine::RuntimeBackend<State = relocade_machine::RuntimeState<T>>,
>(
    mut backend: B,
    Configuration {
        cpu,
        machine: config,
    }: Configuration,
) -> relocade_machine::Result<relocade_machine::Cpu<B>> {
    let machine = relocade_machine::Machine::new(&mut backend, config)?;
    machine.create_cpu(backend, cpu).await
}
