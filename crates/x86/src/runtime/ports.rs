//! Synchronous port transfers retain their architectural operand width.

use super::*;
use relocade_compiler::{AtLeast, MemoryInt};

#[derive(Clone, Copy)]
pub(super) struct Ports {
    read: Func,
    write: Func,
}

impl Ports {
    pub(super) fn declare(program: &mut Program) -> Self {
        let read = program.import_function(crate::abi::READ_PORT);
        let write = program.import_function(crate::abi::WRITE_PORT);
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
