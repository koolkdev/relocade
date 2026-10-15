//! Read-only host queries retain the architectural widths of each result.

use super::*;
use crate::{DescriptorTable, DescriptorTableRegister, SystemSelector};

#[derive(Clone, Copy)]
pub(super) struct SystemQueries {
    descriptor_table: Func,
    system_selector: Func,
}

impl SystemQueries {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            system_selector: program.import_function(FunctionImport {
                module: "wasm86".into(),
                name: "querySystemSelector".into(),
                signature: Signature {
                    parameters: vec![Type::I32],
                    results: vec![Type::I16],
                },
            }),
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
    pub(crate) fn query_system_selector(
        self,
        body: &mut BlockBuilder<'_>,
        register: SystemSelector,
    ) -> Result<Val<I16>, BuildError> {
        body.call::<I16>(self.system.system_selector, &[(register as u32).into()])
    }

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
