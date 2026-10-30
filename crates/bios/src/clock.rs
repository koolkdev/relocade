//! BIOS timer ticks and INT 1Ah clock reads.

use relocade_machine::{real_mode_interrupt, CallContext, HookResult, MemoryView};

const TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;
const TICKS_PER_DAY: u32 = 0x18_00b0;

pub(super) fn initialize(memory: &mut MemoryView<'_>) {
    memory.write(TICKS, &[0; 5]).expect("BIOS clock data");
}

pub(super) async fn tick<T>(mut call: CallContext<T>) -> HookResult {
    let mut memory = call.memory.view();
    let ticks = memory.read_u32(TICKS).expect("BIOS clock ticks");
    if ticks >= TICKS_PER_DAY - 1 {
        memory.write_u32(TICKS, 0).expect("BIOS clock ticks");
        let midnight = memory
            .read_u8(MIDNIGHT_ROLLOVER)
            .expect("BIOS midnight rollover");
        memory
            .write(MIDNIGHT_ROLLOVER, &[midnight.wrapping_add(1)])
            .expect("BIOS midnight rollover");
    } else {
        memory
            .write_u32(TICKS, ticks + 1)
            .expect("BIOS clock ticks");
    }
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
}

pub(super) async fn read<T>(mut call: CallContext<T>) -> HookResult {
    let mut memory = call.memory.view();
    if call.cpu.ah() == 0 {
        let mut data = [0; 5];
        memory.read(TICKS, &mut data).expect("BIOS clock data");
        let cpu = &mut call.cpu;
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        memory
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        call.cpu.set_ah(0x86);
        real_mode_interrupt::set_carry(&call.cpu, &mut memory, true);
    }
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
}
