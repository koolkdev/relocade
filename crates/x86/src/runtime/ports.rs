//! Synchronous port transfers retain their architectural operand width.

use super::Runtime;
use relocade_compiler::{
    AtLeast, BlockBuilder, BuildError, MemoryInt, Program, Table, Val, I16, I32,
};

#[derive(Clone, Copy)]
pub(super) struct Ports {
    read: Table,
    write: Table,
}

impl Ports {
    pub(super) fn declare(program: &mut Program) -> Self {
        let read = program.import_table(crate::abi::PORT_READS);
        let write = program.import_table(crate::abi::PORT_WRITES);
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
        let value = body.call_indirect::<I32>(
            self.ports.read,
            port.unsigned().extend::<I32>(),
            &[port.into(), T::BYTES.into()],
        )?;
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
        body.call_indirect::<()>(
            self.ports.write,
            port.unsigned().extend::<I32>(),
            &[
                port.into(),
                T::BYTES.into(),
                value.unsigned().extend::<I32>().into(),
            ],
        )
    }
}
