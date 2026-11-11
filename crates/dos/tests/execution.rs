use relocade_dos::{Dos, ProcessStatus, RunError, ServiceError};
use relocade_x86::{Exception, ExecutionExit};

#[test]
fn runner_distinguishes_process_completion_service_errors_and_cpu_exits() {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let result = pollster::block_on(prepared.run_until(
        std::future::pending::<()>(),
        async |runner| {
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
        },
    ));
    assert_eq!(result.unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}

#[test]
fn unhandled_divide_errors_reach_the_host_with_the_dos_fault_frame() {
    let mut machine =
        relocade_machine::Machine::new(relocade_dos::configuration::memory()).unwrap();
    let prepared = machine
        .create_cpu(relocade_dos::configuration::cpu(), None)
        .unwrap();
    let result = pollster::block_on(prepared.run_until(
        std::future::pending::<()>(),
        async |runner| {
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
        },
    ));
    assert_eq!(result.unwrap(), Some(()));
    pollster::block_on(machine.shutdown()).unwrap();
}
