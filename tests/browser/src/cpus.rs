use futures_channel::{mpsc, oneshot};
use futures_util::{future::poll_fn, StreamExt};
use relocade_machine::{worker::Thread, Cpu, CpuConfig, Hook, HookResult, MachineConfig, Memory};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};
use std::{cell::Cell, rc::Rc};

struct Input {
    incoming: mpsc::UnboundedReceiver<u16>,
    waiting: Option<oneshot::Sender<()>>,
    completed: Rc<Cell<u32>>,
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn cpu_worker() -> Result<(), String> {
    let config = MachineConfig::new(
        0x3000,
        PhysicalMemoryMap::new([(
            0..=0xfff,
            PhysicalMapping::Ram {
                backing_offset: 0x1000,
            },
        )])
        .unwrap(),
    )
    .unwrap();
    let cpu_config = CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )
    .unwrap();
    let memory = Memory::new(config.clone()).map_err(|error| error.to_string())?;
    let mut view = memory.view();
    // MOV [0204],AX; JMP 0303. Execution resumes here after the input hook.
    view.write(0x300, &[0xa3, 4, 2, 0xeb, 0xfe]).unwrap();
    let worker = Thread::new();
    let (commands, incoming) = mpsc::unbounded();
    let (waiting, started) = oneshot::channel();
    let task = worker.run_send(memory.backing().clone(), move |backing| async move {
        let memory = Memory::with_backing(config, backing).unwrap();
        let input = Input {
            incoming,
            waiting: Some(waiting),
            completed: Rc::new(Cell::new(0)),
        };
        let mut cpu = Cpu::new(memory, cpu_config, input).await.unwrap();
        cpu.install_hooks([Hook::<Input>::new(0x100, async |call| {
            let cpu = &mut call.cpu;
            let memory = &mut call.memory;
            let input = &mut *call.state;
            memory.write_u8(0x200, 0x11).unwrap();
            let value = poll_fn(|context| {
                let poll = input.incoming.poll_next_unpin(context);
                if poll.is_pending() {
                    // The caller sends only after this task registers its waker.
                    if let Some(waiting) = input.waiting.take() {
                        waiting.send(()).unwrap();
                    }
                }
                poll
            })
            .await
            .unwrap();
            cpu.set_ax(value + memory.read_u16(0x202).unwrap());
            cpu.set_eip(0x300);
            input.completed.set(input.completed.get() + 1);
            HookResult::Continue
        })])
        .await
        .unwrap();
        assert_eq!(cpu.run(1).await, ExecutionExit::BudgetExhausted);
        let ax = cpu.context().cpu.ax();
        (ax, cpu.state().completed.get())
    });

    started.await.map_err(|error| error.to_string())?;
    assert_eq!(view.read_u8(0x200).unwrap(), 0x11);
    view.write_u16(0x202, 0x30).unwrap();
    commands.unbounded_send(0x12).unwrap();
    drop(commands);
    assert_eq!(task.await.map_err(|error| error.to_string())?, (0x42, 1));
    drop(worker);
    assert_eq!(view.read_u16(0x204).unwrap(), 0x42);

    // A later worker imports the same allocation and starts with private CPU state.
    let config = memory.configuration().clone();
    let worker = Thread::new();
    let task = worker.run_send(memory.backing().clone(), move |backing| async move {
        let memory = Memory::with_backing(config, backing).unwrap();
        let mut cpu = Cpu::new(memory, cpu_config, ()).await.unwrap();
        let context = cpu.context();
        (context.cpu.ax(), context.memory.read_u16(0x204).unwrap())
    });
    assert_eq!(task.await.map_err(|error| error.to_string())?, (0, 0x42));
    drop(worker);
    assert_eq!(view.read_u16(0x204).unwrap(), 0x42);
    Ok(())
}
