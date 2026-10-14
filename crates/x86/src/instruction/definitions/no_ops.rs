use super::*;

instruction_families! {
    NOP {
        execute: nop;
        forms {
            0x0F 0x1F /0 => no_operands();
        }
    }
    // These obsolete coprocessor controls are integer NOPs on 32-bit x87 FPUs.
    // They neither wait for exceptions nor update x87 instruction metadata.
    FENI_FDISI_FSETPM {
        execute: nop;
        forms {
            0xDB @ 0xE0 => no_operands();
            0xDB @ 0xE1 => no_operands();
            0xDB @ 0xE4 => no_operands();
        }
    }
    PAUSE {
        execute: nop;
        forms {
            F3 0x90 => no_operands();
        }
    }
}

// PAUSE permits a zero-length delay, so it shares NOP's empty semantic body.
fn nop(_execution: &mut ExecutionBuilder<'_, '_>) -> Result<(), BuildError> {
    Ok(())
}
