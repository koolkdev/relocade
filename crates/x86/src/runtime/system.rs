//! Read-only host queries retain the architectural widths of each result.

use super::*;
use crate::{DescriptorTable, DescriptorTableRegister};

#[derive(Clone, Copy)]
pub(super) struct SystemQueries {
    descriptor_table: Func,
}

impl SystemQueries {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            descriptor_table: program.import_function(FunctionImport {
                module: "wasm86".into(),
                name: "queryDescriptorTable".into(),
                signature: Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I32, Type::I16],
                },
            }),
        }
    }
}

impl Runtime {
    pub(crate) fn query_descriptor_table(
        self,
        body: &mut BlockBuilder<'_>,
        table: DescriptorTable,
    ) -> Result<DescriptorTableRegister<Val<I32>, Val<I16>>, BuildError> {
        let (base, limit) =
            body.call::<(I32, I16)>(self.system.descriptor_table, &[(table as u32).into()])?;
        Ok(DescriptorTableRegister { base, limit })
    }
}
