mod common;
#[path = "bios/keyboard.rs"]
mod keyboard;
#[path = "bios/screen.rs"]
mod screen;

use relocade_x86::ExecutionExit;

async fn check_bios_clock() {
    let mut dos = common::dos().await;
    // INT 1Ah followed by JMP $ exposes the returned registers to the host.
    dos.load_com(&[0xcd, 0x1a, 0xeb, 0xfe], Default::default())
        .unwrap();
    let mut initial = [0xff; 5];
    dos.runtime()
        .machine()
        .memory()
        .read(0x46c, &mut initial)
        .unwrap();
    assert_eq!(initial, [0; 5]);
    dos.runtime()
        .machine()
        .memory()
        .write(0x46c, &[0x78, 0x56, 0x12, 0, 1])
        .unwrap();
    for rollover in [1, 0] {
        {
            let mut machine = dos.runtime().machine();
            let mut cpu = machine.cpu();
            cpu.set_eip(0x100);
            cpu.set_ax(0);
            cpu.set_bx(0xabcd);
            cpu.set_cx(0xffff);
            cpu.set_dx(0xffff);
            cpu.set_eflags(0x0a57); // OF, IF, ZF, AF, PF and CF.
        }
        assert_eq!(
            dos.runtime().run(10).unwrap(),
            ExecutionExit::BudgetExhausted
        );
        let cpu = dos.runtime().machine().cpu().snapshot();
        assert_eq!(cpu.registers.eax as u16, rollover);
        assert_eq!(cpu.registers.ebx as u16, 0xabcd);
        assert_eq!(cpu.registers.ecx as u16, 0x12);
        assert_eq!(cpu.registers.edx as u16, 0x5678);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
        assert_eq!(cpu.flags.eflags(), 0x0a57); // Clock reads preserve flags.
        let mut data = [0; 5];
        dos.runtime()
            .machine()
            .memory()
            .read(0x46c, &mut data)
            .unwrap();
        assert_eq!(data, [0x78, 0x56, 0x12, 0, 0]);
    }
    {
        let mut machine = dos.runtime().machine();
        let mut cpu = machine.cpu();
        cpu.set_eip(0x100);
        cpu.set_ax(0xff42);
        cpu.set_eflags(0x0a56); // Same flags, with carry clear.
    }
    assert_eq!(
        dos.runtime().run(10).unwrap(),
        ExecutionExit::BudgetExhausted
    );
    let cpu = dos.runtime().machine().cpu().snapshot();
    assert_eq!(cpu.registers.eax as u16, 0x8642);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // Unsupported calls set only carry.
    assert_eq!(dos.exit_status(), None);
}

#[test]
fn bios_reads_ticks_and_consumes_midnight_rollover() {
    pollster::block_on(check_bios_clock());
}
