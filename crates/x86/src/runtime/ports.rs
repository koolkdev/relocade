//! Synchronous port transfers retain their architectural operand width.

use super::*;
use wasm86_compiler::{AtLeast, MemoryInt};

#[derive(Clone, Copy)]
pub(super) struct Ports {
    read: Func,
    write: Func,
}

impl Ports {
    pub(super) fn declare(program: &mut Program) -> Self {
        let read = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "readPort".into(),
            signature: Signature {
                parameters: vec![Type::I16, Type::I32],
                results: vec![Type::I32],
            },
        });
        let write = program.import_function(FunctionImport {
            module: "wasm86".into(),
            name: "writePort".into(),
            signature: Signature {
                parameters: vec![Type::I16, Type::I32, Type::I32],
                results: vec![],
            },
        });
        Self { read, write }
    }
}

impl Runtime {
    pub(crate) fn read_port<T: MemoryInt>(
        self,
        body: &mut BlockBuilder<'_>,
        port: &Val<I16>,
    ) -> Result<Val<T>, BuildError>
    where
        I32: AtLeast<T>,
    {
        let value = body.call::<I32>(self.ports.read, &[port.into(), T::BYTES.into()])?;
        Ok(value.truncate::<T>())
    }

    pub(crate) fn write_port<T: MemoryInt>(
        self,
        body: &mut BlockBuilder<'_>,
        port: &Val<I16>,
        value: &Val<T>,
    ) -> Result<(), BuildError>
    where
        I32: AtLeast<T>,
    {
        body.call::<()>(
            self.ports.write,
            &[
                port.into(),
                T::BYTES.into(),
                value.unsigned().extend::<I32>().into(),
            ],
        )
    }
}
