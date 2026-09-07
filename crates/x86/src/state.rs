use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryImport, Program, Val, I32};

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Gpr32 {
    Eax,
    Ecx,
    Edx,
    Ebx,
    Esp,
    Ebp,
    Esi,
    Edi,
}

fn register_offset(register: Gpr32) -> u32 {
    match register {
        Gpr32::Eax => 24,
        Gpr32::Ecx => 28,
        Gpr32::Edx => 32,
        Gpr32::Ebx => 36,
        Gpr32::Esp => 40,
        Gpr32::Ebp => 44,
        Gpr32::Esi => 48,
        Gpr32::Edi => 52,
    }
}

const EIP_OFFSET: u32 = 56;
const INSTRUCTION_COUNT_OFFSET: u32 = 144;

pub(super) struct State {
    memory: Mem,
    registers: Vec<(Gpr32, Val<I32>)>,
}

impl State {
    pub(super) fn new(program: &mut Program) -> Self {
        Self {
            memory: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "cpuState".into(),
                minimum: 1,
                maximum: None,
            }),
            registers: Vec::new(),
        }
    }

    pub(super) fn write_register(&mut self, register: Gpr32, value: &Val<I32>) {
        // Replacing in place keeps the first-write order while discarding earlier values.
        if let Some((_, current)) = self.registers.iter_mut().find(|(key, _)| *key == register) {
            *current = value.clone();
        } else {
            self.registers.push((register, value.clone()));
        }
    }

    pub(super) fn publish(
        self,
        body: &mut FunctionBuilder<'_>,
        next_eip: &Val<I32>,
        completed: u32,
    ) -> Result<(), BuildError> {
        for (register, value) in self.registers {
            body.store(self.memory, register_offset(register), &value)?;
        }
        body.store(self.memory, EIP_OFFSET, next_eip)?;
        let count = body.load::<I32>(self.memory, INSTRUCTION_COUNT_OFFSET)?;
        body.store(self.memory, INSTRUCTION_COUNT_OFFSET, &count.add(completed))
    }
}
