mod common;

use relocade_x86::ExecutionExit;

fn out_word(code: &mut Vec<u8>, port: u16, value: u16) {
    code.push(0xba); // MOV DX, port
    code.extend(port.to_le_bytes());
    code.push(0xb8); // MOV AX, value
    code.extend(value.to_le_bytes());
    code.push(0xef); // OUT DX, AX
}

fn dispi(code: &mut Vec<u8>, index: u16, value: u16) {
    out_word(code, 0x1ce, index);
    out_word(code, 0x1cf, value);
}

fn graphics_program() -> Vec<u8> {
    let mut code = Vec::new();
    for (index, value) in [(1, 640), (2, 480), (3, 8), (4, 1)] {
        dispi(&mut code, index, value);
    }
    code.extend([0xb8, 0x00, 0xa0, 0x8e, 0xc0]); // MOV AX,A000h; MOV ES,AX
    code
}

#[test]
fn guest_word_io_banking_and_bios_palette_share_the_production_device() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let mut code = graphics_program();
        // BIOS AX=1010h sets DAC entry BX with DH/CH/CL as RGB.
        code.extend([
            0xb8, 0x10, 0x10, 0xbb, 0x2a, 0x00, 0xba, 0x00, 0x3f, 0xb9, 0x07, 0x05, 0xcd, 0x10,
        ]);
        code.extend([0x26, 0xc7, 0x06, 0xfe, 0xff, 0x34, 0x12]); // Bank 0 ES:[FFFE] = 1234h
        dispi(&mut code, 5, 63);
        code.extend([0x26, 0xc7, 0x06, 0xfe, 0xff, 0xef, 0xbe]); // ES:[FFFE] = BEEFh
        code.extend([0x26, 0x8b, 0x1e, 0xfe, 0xff]); // MOV BX, ES:[FFFE]
        dispi(&mut code, 5, 0);
        code.extend([0x26, 0x8b, 0x0e, 0xfe, 0xff]); // MOV CX, ES:[FFFE]
        let completion_eip = 0x100 + code.len() as u32;
        code.extend([0xeb, 0xfe]); // JMP $
        dos.launch_com(&code, Default::default()).unwrap();
        assert_eq!(
            common::run_until(dos.cpu(), completion_eip).await,
            ExecutionExit::Interrupted
        );
        let cpu = dos.cpu().context().cpu.snapshot();
        assert_eq!(cpu.registers.ebx as u16, 0xbeef);
        assert_eq!(cpu.registers.ecx as u16, 0x1234);
        assert_eq!(dos.vga().borrow().snapshot().dac.colors[42], [63, 5, 7]);
    });
}

#[test]
fn bios_leaves_svga_only_for_a_valid_legacy_mode_request() {
    pollster::block_on(async {
        for request in [0x7f, 0x03] {
            let mut dos = common::dos().await;
            let mut code = graphics_program();
            dispi(&mut code, 5, 63);
            code.extend([0x26, 0xc7, 0x06, 0xfe, 0xff, 0xef, 0xbe]);
            code.extend([0xb8, request, 0x00, 0xcd, 0x10]); // BIOS mode set.
            if request == 3 {
                code.extend([0xb8, b'A', 0x0e, 0xcd, 0x10]); // Text teletype.
            }
            let completion_eip = 0x100 + code.len() as u32;
            code.extend([0xeb, 0xfe]); // JMP $
            dos.launch_com(&code, Default::default()).unwrap();
            assert_eq!(
                common::run_until(dos.cpu(), completion_eip).await,
                ExecutionExit::Interrupted
            );
            let context = dos.cpu().context();
            context.io.write_port(0x1ce, 2, 4);
            assert_eq!(context.io.read_port(0x1cf, 2), u32::from(request != 3));
            if request == 3 {
                assert_eq!(context.memory.read_u16(0xb8000).unwrap(), 0x0741);
                let display = dos.vga().borrow().snapshot();
                assert_eq!(display.geometry().size, [720, 400]);
                assert_eq!(display.plane_byte(2, 65 * 32 + 2), 0x38);
            } else {
                assert_eq!(context.memory.read_u16(0xafffe).unwrap(), 0xbeef);
                assert_eq!(context.cpu.snapshot().flags.eflags() & 1, 1);
            }
        }
    });
}
