use crate::backend;
use futures_channel::{mpsc, oneshot};
use futures_util::{future::poll_fn, StreamExt};
use relocade_machine::{MachineConfig, ReadMemory};
use relocade_runtime::{Hook, Runtime, RuntimeState};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::Cell, future::Future, ops::ControlFlow, pin::Pin, rc::Rc};
use web_thread::Thread;

const PROGRAM: u32 = 0x100;
const READ_INPUT: u32 = 0x200;
const MARKER: u32 = 0x400;
const INSTRUCTION_BUDGET: u64 = 256;

enum Command {
    ReadInput,
}

#[derive(Debug, PartialEq)]
enum Event {
    Running,
    WaitingForInput,
}

struct State {
    input: Option<oneshot::Receiver<u8>>,
    events: mpsc::UnboundedSender<Event>,
    // Created by the CPU task, never sent to another thread.
    calls: Rc<Cell<u32>>,
}

async fn run(
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: mpsc::UnboundedSender<Event>,
    input: oneshot::Receiver<u8>,
) -> Result<CpuState, String> {
    let state = RuntimeState::new(State {
        input: Some(input),
        events: events.clone(),
        calls: Rc::new(Cell::new(0)),
    });
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: PROGRAM,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
        0x10000,
        PhysicalMemoryMap::new([(0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap(),
    )
    .unwrap();
    let mut runtime = Runtime::new(backend(state)?, config)
        .await
        .map_err(|error| error.to_string())?;
    runtime
        .machine()
        .memory()
        .write(
            PROGRAM,
            &[
                0xc6, 0x06, 0, 4, 1,    // MOV byte [0400],1
                0x40, // INC AX
                0xeb, 0xf8, // JMP 0100
            ],
        )
        .unwrap();
    runtime
        .install_hooks([Hook::<State>::new(READ_INPUT, |call| {
            let mut input = call.state_mut().input.take().unwrap();
            call.defer(async move |call| {
                let calls = Rc::clone(&call.state().calls);
                calls.set(calls.get() + 1);
                let mut announced_wait = false;
                let byte = poll_fn(|context| {
                    let poll = Pin::new(&mut input).poll(context);
                    if poll.is_pending() && !announced_wait {
                        // Send input only after the receiver has registered its
                        // waker. Return Pending to exercise actual suspension.
                        call.state()
                            .events
                            .unbounded_send(Event::WaitingForInput)
                            .unwrap();
                        announced_wait = true;
                    }
                    poll
                })
                .await
                .unwrap();
                // This same non-Send handler and its CPU borrow survive the wait.
                assert_eq!(calls.get(), 1);
                call.cpu().set_ax(u16::from(byte));
                ControlFlow::Break(ExecutionExit::HostReturn)
            })
        })])
        .await
        .map_err(|error| error.to_string())?;

    assert_eq!(
        runtime
            .run(INSTRUCTION_BUDGET)
            .map_err(|error| error.to_string())?,
        ExecutionExit::BudgetExhausted
    );
    events.unbounded_send(Event::Running).unwrap();
    loop {
        match commands.try_recv() {
            Ok(Command::ReadInput) => runtime.machine().cpu().set_eip(READ_INPUT),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Closed) => return Err("CPU command channel closed".into()),
        }
        // Ordinary input goes straight to the pending handler's Rust channel.
        // Keep this future alive until it completes; do not cancel it to deliver input.
        match runtime
            .run_async(INSTRUCTION_BUDGET)
            .await
            .map_err(|error| error.to_string())?
        {
            ExecutionExit::BudgetExhausted => {}
            ExecutionExit::HostReturn => {
                assert_eq!(runtime.shared_memory().read_u8(MARKER).unwrap(), 1);
                return Ok(runtime.machine().cpu().snapshot());
            }
            exit => return Err(format!("unexpected guest exit: {exit:?}")),
        }
    }
}

pub(super) async fn check() -> Result<(), String> {
    let thread = Thread::new();
    let (commands, incoming) = mpsc::unbounded();
    let (events, mut observed) = mpsc::unbounded();
    let (input, receiver) = oneshot::channel();
    let task = thread.run_send((), move |()| run(incoming, events, receiver));

    assert_eq!(observed.next().await, Some(Event::Running));
    commands.unbounded_send(Command::ReadInput).unwrap();
    assert_eq!(observed.next().await, Some(Event::WaitingForInput));
    input.send(42).unwrap();
    let cpu = task.await.map_err(|error| error.to_string())??;
    assert_eq!(cpu.registers.eax, 42);
    assert!(cpu.instruction_count >= INSTRUCTION_BUDGET);
    drop(thread);
    Ok(())
}
