use super::*;

struct ForwardingDevice;

impl Device for ForwardingDevice {}

async fn check_forwarding() {
    for compiled in [false, true] {
        let target = RecordingDevice::new(0);
        let io = IoBus::default();
        io.connect(target.clone(), |io| {
            RecordingDevice::bind(io, &target, 0x3d4);
        });
        io.connect(Rc::new(RefCell::new(ForwardingDevice)), |io| {
            io.bind_mmio(
                0x3000..=0x4fff,
                &MmioRead::new(|mut context, _, bytes| context.read_port(0x3d4, bytes).into()),
                &MmioWrite::new(|mut context, _, bytes, value| {
                    context.write_port(0x3d4, bytes, value as u32);
                }),
            );
            io.bind(
                0x3d5,
                &PortRead::new(|mut context, _, bytes| context.read_port(0x3d4, bytes)),
                &PortWrite::new(|mut context, _, bytes, value| {
                    context.write_port(0x3d4, bytes, value)
                }),
            );
        });
        // Host calls receive a real scope before the CPU exists, too.
        io.with_context(|mut context| {
            context.write_port(0x3d5, 2, 0x1234);
            assert_eq!(context.read_mmio(0x3000, 2), 0x1234);
            context.write_mmio(0x3000, 2, 0x2468);
            assert_eq!(context.read_port(0x3d5, 2), 0x2468);
        });
        target.borrow_mut().take_transfers();

        let compiler = Compiler::new(ExecutionProfile::Real16);
        let config = support::configuration(
            compiler.target(),
            CpuState {
                eip: 0x100,
                ..CpuState::real_mode()
            },
            0x5000,
            map(),
        )
        .unwrap();
        let memory = Memory::new(config.machine).unwrap();
        let code = [
            0xba, 0xd5, 0x03, // MOV DX,03D5
            0xb8, 0x78, 0x56, // MOV AX,5678
            0xef, // OUT DX,AX
            0xb8, 0, 0,    // MOV AX,0
            0xed, // IN AX,DX
            0xa3, 0, 0x30, // MOV [3000],AX
            0xa1, 0, 0x30, // MOV AX,[3000]
            0xeb, 0, // JMP +0
        ];
        memory.view().write(0x100, &code).unwrap();
        let mut cpu = Cpu::new(memory, config.cpu, io.clone()).await.unwrap();
        if compiled {
            let snapshots = [0, 7, 11].map(|offset| BlockSnapshot {
                start_eip: 0x100 + offset as u32,
                bytes: &code[offset..],
                instruction_limit: 8,
            });
            let module = compiler.compile_blocks(&snapshots).unwrap();
            let entries: Vec<_> = module
                .entries
                .iter()
                .map(|entry| DispatchEntry {
                    address: entry.start_eip,
                    export: &entry.export,
                })
                .collect();
            cpu.install_module(&module.bytes, &entries).await.unwrap();
        }
        assert_eq!(
            support::run_until(&mut cpu, 0x100 + code.len() as u32).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(cpu.context().cpu.ax(), 0x5678);
        assert_eq!(
            target.borrow_mut().take_transfers(),
            [
                Transfer::PortWrite {
                    port: 0x3d4,
                    bytes: 2,
                    value: 0x5678
                },
                Transfer::PortRead {
                    port: 0x3d4,
                    bytes: 2
                },
                Transfer::PortWrite {
                    port: 0x3d4,
                    bytes: 2,
                    value: 0x5678
                },
                Transfer::PortRead {
                    port: 0x3d4,
                    bytes: 2
                },
            ]
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_forward_io_through_their_supplied_context() {
    pollster::block_on(check_forwarding());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_forward_io_through_their_supplied_context() {
    check_forwarding().await;
}
