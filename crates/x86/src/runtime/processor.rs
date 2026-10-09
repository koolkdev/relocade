//! Queries of the host's virtual processor identity and clock.

use super::*;
use wasm86_compiler::I64;

#[derive(Clone, Copy)]
pub(super) struct Processor {
    cpuid: Func,
    timestamp_counter: Func,
}

pub(crate) struct CpuidValues {
    pub(crate) eax: Val<I32>,
    pub(crate) ebx: Val<I32>,
    pub(crate) ecx: Val<I32>,
    pub(crate) edx: Val<I32>,
}

impl Processor {
    pub(super) fn declare(program: &mut Program) -> Self {
        let cpuid = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "cpuid".into(),
            signature: Signature {
                parameters: vec![Type::I32, Type::I32],
                results: vec![Type::I32; 4],
            },
        });
        let timestamp_counter = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "readTimestampCounter".into(),
            signature: Signature {
                parameters: vec![],
                results: vec![Type::I64],
            },
        });
        Self {
            cpuid,
            timestamp_counter,
        }
    }
}

impl Runtime {
    pub(crate) fn read_timestamp_counter(
        self,
        body: &mut BlockBuilder<'_>,
    ) -> Result<Val<I64>, BuildError> {
        body.call::<I64>(self.processor.timestamp_counter, &[])
    }

    pub(crate) fn cpuid(
        self,
        body: &mut BlockBuilder<'_>,
        leaf: &Val<I32>,
        subleaf: &Val<I32>,
    ) -> Result<CpuidValues, BuildError> {
        let (eax, ebx, ecx, edx) = body
            .call::<(I32, I32, I32, I32)>(self.processor.cpuid, &[leaf.into(), subleaf.into()])?;
        Ok(CpuidValues { eax, ebx, ecx, edx })
    }
}
