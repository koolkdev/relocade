use relocade_dos::{Dos, ProcessStatus, RunError, ServiceError};
use relocade_machine::{timer_service, Runner};
use relocade_x86::{Exception, ExecutionExit};

#[test]
fn runner_distinguishes_process_completion_service_errors_and_cpu_exits() {
    let (clock, timers) = timer_service();
    let host = std::thread::spawn(|| pollster::block_on(timers.run()));
    pollster::block_on(Runner::with_clock(clock).run(async |runner| {
        let mut dos = Dos::new(runner).await.unwrap();
        assert_eq!(dos.run(runner).await, Err(RunError::NoProcess));
        dos.launch_com(&[0xb8, 7, 0x4c, 0xcd, 0x21], Default::default())
            .unwrap();
        assert_eq!(dos.run(runner).await, Ok(7));
        let count = dos.cpu().context().cpu.instruction_count();
        assert_eq!(dos.run(runner).await, Ok(7));
        assert_eq!(dos.cpu().context().cpu.instruction_count(), count);

        dos.launch_com(&[0xb4, 0xfe, 0xcd, 0x21], Default::default())
            .unwrap();
        assert_eq!(
            dos.run(runner).await,
            Err(RunError::Service(ServiceError::UnsupportedFunction(0xfe)))
        );
        dos.launch_com(&[0x0f, 0x0b], Default::default()).unwrap();
        assert_eq!(
            dos.run(runner).await,
            Err(RunError::Execution(ExecutionExit::Exception(
                Exception::InvalidOpcode
            )))
        );
        assert_eq!(dos.process().unwrap().status(), ProcessStatus::Active);
    }));
    host.join().unwrap();
}
