//! Queries of the host's virtual processor identity.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct Processor {
    cpuid: Func,
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
        Self { cpuid }
    }
}

impl Runtime {
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
