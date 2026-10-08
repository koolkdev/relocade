//! Port addresses and operand widths are independent of each other.

use super::*;
use crate::register::RegisterType;

instruction_families! {
    IN {
        execute: input;
        effects: [port_io];
        availability: IoPrivileged;
        forms {
            0xE4 => byte(accumulator, imm8);
            0xE5 => word_or_dword(accumulator, imm8);
            0xEC => byte(accumulator, DX);
            0xED => word_or_dword(accumulator, DX);
        }
    }
    OUT {
        execute: output;
        effects: [port_io];
        availability: IoPrivileged;
        forms {
            0xE6 => byte(imm8, accumulator);
            0xE7 => word_or_dword(imm8, accumulator);
            0xEE => byte(DX, accumulator);
            0xEF => word_or_dword(DX, accumulator);
        }
    }
}

fn input<T: RegisterType, P: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    destination: TypedLocation<T>,
    port: Input<P>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T> + AtLeast<P>,
    I16: AtLeast<P>,
{
    let port = port.read(execution)?.unsigned().extend::<I16>();
    let value = execution.read_port::<T>(&port)?;
    destination.write(execution, value)
}

fn output<T: RegisterType, P: RegisterType>(
    execution: &mut ExecutionBuilder<'_, '_>,
    port: Input<P>,
    source: Input<T>,
) -> Result<(), BuildError>
where
    I32: AtLeast<T> + AtLeast<P>,
    I16: AtLeast<P>,
{
    let port = port.read(execution)?.unsigned().extend::<I16>();
    let value = source.read(execution)?;
    execution.write_port(&port, &value)
}
