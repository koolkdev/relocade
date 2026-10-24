#![allow(dead_code)]
use crate::runtime::RuntimeState;
use crate::{CpuConfig, CpuContext, MachineConfig, MemoryAccess, MemoryStorage};
use relocade_x86::ExecutionContext;
use std::{cell::RefCell, rc::Rc};

pub struct Storage {
    pub cpu: CpuImage,
    bytes: Vec<u8>,
    state: RuntimeState,
}

pub struct CpuImage {
    pub context: Rc<RefCell<[u8; ExecutionContext::BYTE_LEN]>>,
    pub cpu_reads: RefCell<Vec<(u32, usize)>>,
}

impl MemoryStorage for CpuImage {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        self.cpu_reads.borrow_mut().push((offset, bytes.len()));
        let offset = offset as usize;
        bytes.copy_from_slice(&self.context.borrow()[offset..][..bytes.len()]);
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        let offset = offset as usize;
        self.context.borrow_mut()[offset..][..bytes.len()].copy_from_slice(bytes);
    }
}

impl Storage {
    pub fn new(cpu: &CpuConfig, config: &MachineConfig) -> Self {
        Self {
            cpu: CpuImage {
                context: Rc::new(RefCell::new(
                    ExecutionContext {
                        cpu: *cpu.cpu(),
                        instruction_target: 998,
                        cpu_model: cpu.execution_target().cpu_model,
                    }
                    .to_bytes(),
                )),
                cpu_reads: RefCell::new(Vec::new()),
            },
            bytes: vec![0; config.memory_size() as usize],
            state: RuntimeState::default(),
        }
    }

    pub fn context<'a>(&'a mut self, config: &'a Configuration) -> CpuContext<'a> {
        CpuContext::new(
            &config.cpu,
            &config.machine,
            MemoryAccess::from_storage(&mut self.cpu),
            MemoryAccess::from_bytes(&mut self.bytes),
            &mut self.state,
        )
    }
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
) -> Result<Configuration, crate::ConfigurationError> {
    Ok(Configuration {
        cpu: CpuConfig::new(target, cpu)?,
        machine: MachineConfig::new(size, map)?,
    })
}
