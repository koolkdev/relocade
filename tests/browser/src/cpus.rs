use futures_channel::oneshot;
use relocade_machine::{
    worker::{channel, Thread},
    Cpu, CpuConfig, MachineConfig, Memory,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionExit, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Segments,
};

fn configuration(eip: u32) -> CpuConfig {
    CpuConfig::new(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip,
            segments: Segments::real_mode(),
            ..CpuState::default()
        },
    )
    .unwrap()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn shared_cpu_memory() -> Result<(), String> {
    let creator = Thread::new();
    let receiver = Thread::new();
    let (outgoing, incoming) = channel::<Memory>().map_err(|error| error.to_string())?;
    let (ready, attached) = oneshot::channel();
    let (resume, creator_gone) = oneshot::channel();

    let first = creator.run(outgoing, move |outgoing| async move {
        let map = PhysicalMemoryMap::new([
            (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
            (
                0x100000..=0x100fff,
                PhysicalMapping::Rom {
                    backing_offset: 0xf000,
                },
            ),
        ])
        .unwrap();
        let memory = Memory::new(MachineConfig::new(0x10003, map).unwrap()).unwrap();
        let mut cpu = Cpu::new(memory, configuration(0x100), ()).await.unwrap();
        let memory = cpu.memory();
        // MOV AX,1234; MOV [0200],AX; JMP next.
        memory
            .view()
            .write(0x100, &[0xb8, 0x34, 0x12, 0xa3, 0, 2, 0xeb, 0])
            .unwrap();
        // MOV AX,[0200]; INC AX; MOV [0200],AX; JMP 0120.
        memory
            .view()
            .write(0x120, &[0xa1, 0, 2, 0x40, 0xa3, 0, 2, 0xeb, 0xf7])
            .unwrap();
        assert_eq!(cpu.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        outgoing.send(memory.clone()).unwrap();
        attached.await.unwrap();
        assert_eq!(cpu.context().cpu.ax(), 0x1234);
        memory
    });
    let second = receiver.run_send(incoming, move |mut incoming| async move {
        let memory = incoming.recv().await.unwrap().unwrap();
        // Transport preserves configured capacity and pages currently hidden by A20.
        assert_eq!(memory.configuration().memory_size(), 0x10003);
        assert_eq!(
            memory.view().read_backing(0x10003, &mut [0]),
            Err(relocade_machine::MemoryError::BackingRange)
        );
        let mut map = memory.configuration().memory_map().clone();
        assert_eq!(
            map.get(0x100000),
            PhysicalMapping::Ram { backing_offset: 0 }
        );
        map.set_a20_enabled(true);
        assert_eq!(
            map.get(0x100000),
            PhysicalMapping::Rom {
                backing_offset: 0xf000
            }
        );
        let mut cpu = Cpu::new(memory, configuration(0x120), ()).await.unwrap();
        assert_eq!(cpu.context().cpu.ax(), 0);
        assert_eq!(cpu.context().cpu.instruction_count(), 0);
        ready.send(()).unwrap();
        creator_gone.await.unwrap();
        assert_eq!(cpu.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.context().cpu.ax(), 0x1235);
        assert_eq!(cpu.memory().view().read_u16(0x200).unwrap(), 0x1235);
    });

    let owner = first.await.map_err(|error| error.to_string())?;
    drop(creator);
    resume.send(()).unwrap();
    second.await.map_err(|error| error.to_string())?;
    drop(receiver);

    // Retained memory stays usable after both original workers have gone.
    let late = Thread::new();
    late.run_send(owner.clone(), move |retained| async move {
        let mut cpu = Cpu::new(retained, configuration(0x120), ()).await.unwrap();
        assert_eq!(cpu.context().cpu.ax(), 0);
        assert_eq!(cpu.run(1).unwrap(), ExecutionExit::BudgetExhausted);
        assert_eq!(cpu.context().cpu.ax(), 0x1236);
    })
    .await
    .map_err(|error| error.to_string())?;
    drop(late);
    assert_eq!(owner.view().read_u16(0x200).unwrap(), 0x1236);
    Ok(())
}
