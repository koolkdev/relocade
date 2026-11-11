#[path = "../../machine/tests/support/compilation.rs"]
mod compilation;

use relocade_dos::{Dos, ProcessStatus, RunError, ServiceError};
use relocade_x86::{Exception, ExecutionExit};

#[test]
fn runner_distinguishes_process_completion_service_errors_and_cpu_exits() {
    pollster::block_on(compilation::run(execution_exits));
}

async fn execution_exits(compiler: &relocade_machine::CompilerService) {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory(), compiler).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu())
        .unwrap();
    let result = prepared
        .run_until(std::future::pending::<()>(), async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            assert_eq!(dos.run().await, Err(RunError::NoProcess));
            dos.launch_com(&[0xb8, 7, 0x4c, 0xcd, 0x21], Default::default())
                .unwrap();
            assert_eq!(dos.run().await, Ok(7));
            let count = dos.cpu().context().cpu.instruction_count();
            assert_eq!(dos.run().await, Ok(7));
            assert_eq!(dos.cpu().context().cpu.instruction_count(), count);

            dos.launch_com(&[0xb4, 0x2a, 0xcd, 0x21], Default::default())
                .unwrap();
            assert_eq!(
                dos.run().await,
                Err(RunError::Service(ServiceError::UnsupportedFunction(0x2a)))
            );
            // MOV AX,[10000] reports an undelivered segment-limit fault.
            dos.launch_com(&[0x67, 0xa1, 0, 0, 1, 0], Default::default())
                .unwrap();
            assert_eq!(
                dos.run().await,
                Err(RunError::Execution(ExecutionExit::Exception(
                    Exception::GeneralProtection { error_code: 0 }
                )))
            );
            assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        })
        .await;
    assert_eq!(result.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}

#[test]
fn unhandled_divide_errors_reach_the_host_with_the_dos_fault_frame() {
    pollster::block_on(compilation::run(divide_error));
}

async fn divide_error(compiler: &relocade_machine::CompilerService) {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory(), compiler).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu())
        .unwrap();
    let result = prepared
        .run_until(std::future::pending::<()>(), async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(&[0x31, 0xdb, 0xf6, 0xf3], Default::default())
                .unwrap(); // XOR BX,BX; DIV BL.
            let state = dos.cpu().cpu_state();
            let program_segment = state.segment(relocade_x86::Segment::Cs).selector;
            assert_eq!(
                dos.run().await,
                Err(RunError::Execution(ExecutionExit::UnhandledException(
                    Exception::DivideError
                )))
            );
            let frame = relocade_machine::real_mode_interrupt::read_frame(
                &state,
                &dos.cpu().memory().view(),
            );
            assert_eq!(
                frame.return_address,
                relocade_machine::FarPointer {
                    segment: program_segment,
                    offset: 0x102,
                }
            );
            assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
        })
        .await;
    assert_eq!(result.unwrap(), Some(()));
    machine.shutdown().await.unwrap();
}
