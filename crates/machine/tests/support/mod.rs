use relocade_machine::{MachineConfig, MachineStorage};
use relocade_x86::ExecutionContext;
use std::{cell::RefCell, mem::offset_of, rc::Rc};

pub struct Storage {
    pub context: Rc<RefCell<[u8; ExecutionContext::BYTE_LEN]>>,
    pub cpu_reads: RefCell<Vec<(u32, usize)>>,
    bytes: Vec<u8>,
    mapping: Vec<u8>,
    descriptors: Vec<u8>,
}

impl MachineStorage for Storage {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.cpu_reads.borrow_mut().push((offset, bytes.len()));
        let offset = offset_of!(ExecutionContext, cpu) + offset as usize;
        bytes.copy_from_slice(&self.context.borrow()[offset..][..bytes.len()]);
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        let offset = offset_of!(ExecutionContext, cpu) + offset as usize;
        self.context.borrow_mut()[offset..][..bytes.len()].copy_from_slice(bytes);
    }
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.bytes[offset as usize..][..bytes.len()]);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.bytes[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    fn read_mapping(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.mapping[offset as usize..][..bytes.len()]);
    }
    fn write_mapping(&mut self, offset: u32, bytes: &[u8]) {
        self.mapping[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    fn read_descriptors(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.descriptors[offset as usize..][..bytes.len()]);
    }
    fn write_descriptors(&mut self, offset: u32, bytes: &[u8]) {
        self.descriptors[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
}

impl Storage {
    pub fn new(config: &MachineConfig) -> Self {
        Self {
            context: Rc::new(RefCell::new(
                ExecutionContext {
                    cpu: *config.cpu(),
                    instruction_target: 998,
                    cpu_model: config.execution_target().cpu_model,
                }
                .to_bytes(),
            )),
            cpu_reads: RefCell::new(Vec::new()),
            bytes: vec![0; config.memory_size() as usize],
            mapping: config.memory_map().to_bytes(),
            descriptors: config.descriptor_memory().to_bytes().to_vec(),
        }
    }
}
