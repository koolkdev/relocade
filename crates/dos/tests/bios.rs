#[path = "bios/cga.rs"]
mod cga;
mod common;
#[path = "bios/fonts.rs"]
mod fonts;
#[path = "bios/geometry.rs"]
mod geometry;
#[path = "bios/graphics.rs"]
mod graphics;
#[path = "bios/graphics_characters.rs"]
mod graphics_characters;
#[path = "bios/graphics_pages.rs"]
mod graphics_pages;
#[path = "bios/keyboard.rs"]
mod keyboard;
#[path = "bios/palette.rs"]
mod palette;
#[path = "bios/pixels.rs"]
mod pixels;
#[path = "bios/planar.rs"]
mod planar;
#[path = "bios/screen.rs"]
mod screen;
#[path = "bios/text_modes.rs"]
mod text_modes;
#[path = "bios/video.rs"]
mod video;

use relocade_dos::ProcessStatus;
use relocade_x86::ExecutionExit;

async fn check_bios_clock() {
    let mut dos = common::dos().await;
    // INT 1Ah followed by JMP $ exposes the returned registers to the host.
    dos.launch_com(&[0xcd, 0x1a, 0xeb, 0xfe], Default::default())
        .unwrap();
    let mut initial = [0xff; 5];
    dos.cpu()
        .context()
        .memory
        .read(0x46c, &mut initial)
        .unwrap();
    assert_eq!(initial, [0; 5]);
    dos.cpu()
        .context()
        .memory
        .write(0x46c, &[0x78, 0x56, 0x12, 0, 1])
        .unwrap();
    for rollover in [1, 0] {
        {
            let mut machine = dos.cpu().context();
            let cpu = &mut machine.cpu;
            cpu.set_eip(0x100);
            cpu.set_ax(0);
            cpu.set_bx(0xabcd);
            cpu.set_cx(0xffff);
            cpu.set_dx(0xffff);
            cpu.set_eflags(0x0a57); // OF, IF, ZF, AF, PF and CF.
        }
        assert_eq!(
            pollster::block_on(dos.cpu().run(10)),
            ExecutionExit::BudgetExhausted
        );
        let cpu = dos.cpu().context().cpu.snapshot();
        assert_eq!(cpu.registers.eax as u16, rollover);
        assert_eq!(cpu.registers.ebx as u16, 0xabcd);
        assert_eq!(cpu.registers.ecx as u16, 0x12);
        assert_eq!(cpu.registers.edx as u16, 0x5678);
        assert_eq!(cpu.registers.esp as u16, 0xfffe);
        assert_eq!(cpu.flags.eflags(), 0x0a57); // Clock reads preserve flags.
        let mut data = [0; 5];
        dos.cpu().context().memory.read(0x46c, &mut data).unwrap();
        assert_eq!(data, [0x78, 0x56, 0x12, 0, 0]);
    }
    {
        let mut machine = dos.cpu().context();
        let cpu = &mut machine.cpu;
        cpu.set_eip(0x100);
        cpu.set_ax(0xff42);
        cpu.set_eflags(0x0a56); // Same flags, with carry clear.
    }
    assert_eq!(
        pollster::block_on(dos.cpu().run(10)),
        ExecutionExit::BudgetExhausted
    );
    let cpu = dos.cpu().context().cpu.snapshot();
    assert_eq!(cpu.registers.eax as u16, 0x8642);
    assert_eq!(cpu.flags.eflags(), 0x0a57); // Unsupported calls set only carry.
    assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
}

#[test]
fn bios_reads_ticks_and_consumes_midnight_rollover() {
    pollster::block_on(check_bios_clock());
}
