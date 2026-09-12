//! Transfers between AH and the low architectural flag byte.

use super::*;
use crate::flags::image;

instruction_families! {
    LAHF {
        execute: load_flags;
        forms {
            0x9F => byte(AH);
        }
    }
    SAHF {
        execute: store_flags;
        forms {
            0x9E => byte(AH);
        }
    }
}

fn load_flags(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<I8>,
) -> Result<(), BuildError> {
    let values = execution.read_flags(image::AH.flags())?;
    destination.write(execution, image::AH.pack(values).truncate::<I8>())
}

fn store_flags(
    execution: &mut ExecutionBuilder<'_, '_>,
    source: Input<I8>,
) -> Result<(), BuildError> {
    let byte = source.read(execution)?;
    execution.write_flags(image::AH.change(&byte.unsigned().extend::<I32>()))
}
