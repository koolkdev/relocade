mod common;
#[path = "../../machine/tests/support/execution.rs"]
mod execution;

use relocade_clock_test_support::ManualClock;
use relocade_machine::{DispatchEntry, IoBus};
use relocade_pic::Pic;
use relocade_ps2::{connect, DATA_PORT, OUTPUT_BUFFER_FULL, STATUS_COMMAND_PORT};
use relocade_x86::{BlockSnapshot, Compiler, ExecutionExit, ExecutionProfile};
use std::future::pending;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn output(io: &IoBus) -> u8 {
    for _ in 0..32 {
        if io.read_port(STATUS_COMMAND_PORT, 1) & u32::from(OUTPUT_BUFFER_FULL) != 0 {
            return io.read_port(DATA_PORT, 1) as u8;
        }
        relocade_machine::yield_now().await;
    }
    panic!("PS/2 firmware did not publish output");
}

async fn guest_port_io() {
    for compiled in [false, true] {
        let clock = ManualClock::default();
        let (runner, _host, machine) = common::runner(clock.clock()).await;
        let result = runner
            .run(pending::<()>(), async |mut runner| {
                let pic = Pic::initialize(&runner).await;
                let (_inputs, controller) = connect(&runner, &pic).await;
                let io = runner.io();
                let code = [
                    0xe5, 0x60, // IN AX,60: wide reads must not consume output.
                    0xe4, 0x64, // IN AL,64: status, without consuming output.
                    0xe4, 0x60, // IN AL,60: consume the data latch.
                    0xe4, 0x64, // IN AL,64: output is now empty.
                    0xb0, 0x20, 0xe7, 0x64, // MOV AL,20; OUT 64,AX: ignored wide write.
                    0xe4, 0x64, // IN AL,64
                    0xb0, 0x20, 0xe6, 0x64, // MOV AL,20; OUT 64,AL: read command byte.
                    0xe4, 0x64, // IN AL,64: input full, last write was a command.
                    0xb0, 0xee, 0xe6, 0x60, // MOV AL,EE; OUT 60,AL: keyboard echo.
                    0xe4, 0x64, // IN AL,64
                ];
                runner.memory().view().write(0x100, &code).unwrap();
                if compiled {
                    let compiler = Compiler::new(ExecutionProfile::Real16);
                    let snapshots =
                        [0, 2, 4, 6, 8, 12, 14, 18, 20, 24].map(|offset| BlockSnapshot {
                            start_eip: 0x100 + offset as u32,
                            bytes: &code[offset..],
                            instruction_limit: if matches!(offset, 8 | 14 | 20) { 2 } else { 1 },
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
                    runner
                        .cpu()
                        .install_module(&module.bytes, &entries)
                        .await
                        .unwrap();
                }

                // Inject an output byte through the same ports used by BIOS.
                io.write_port(STATUS_COMMAND_PORT, 1, 0xd2);
                controller.wait_input_ready().await;
                io.write_port(DATA_PORT, 1, 0x5a);
                controller.wait_input_ready().await;
                assert_eq!(io.read_port(STATUS_COMMAND_PORT, 1), 0x15);

                // No firmware polling between these synchronous guest transfers:
                // the final data write must preserve the pending command.
                for (eip, ax) in [
                    (0x102, 0xffff),
                    (0x104, 0xff15),
                    (0x106, 0xff5a),
                    (0x108, 0xff14),
                    (0x10e, 0xff14),
                    (0x114, 0xff1e),
                    (0x11a, 0xff1e),
                ] {
                    assert_eq!(
                        execution::run_until(runner.cpu(), eip).await,
                        ExecutionExit::Interrupted
                    );
                    assert_eq!(runner.cpu().cpu_state().eax() & 0xffff, ax, "EIP {eip:#x}");
                }
                assert_eq!(output(&io).await, 0x65, "busy data write kept the command");

                // Retry the same data write once the input buffer is available.
                runner.cpu().cpu_state().set_eip(0x114);
                assert_eq!(
                    execution::run_until(runner.cpu(), 0x11a).await,
                    ExecutionExit::Interrupted
                );
                assert_eq!(runner.cpu().cpu_state().eax() & 0xff, 0x16);
                assert_eq!(
                    output(&io).await,
                    0xee,
                    "data write reached keyboard firmware"
                );
            })
            .await;
        assert_eq!(result, Some(()));
        machine.shutdown().await.unwrap();
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn interpreted_and_compiled_controller_ports() {
    pollster::block_on(guest_port_io());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn interpreted_and_compiled_controller_ports() {
    guest_port_io().await;
}
