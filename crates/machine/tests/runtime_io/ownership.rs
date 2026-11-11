use super::*;

async fn check_retained_memory_bus() {
    let device = RecordingDevice::new(0x3000);
    let lifetime = Rc::downgrade(&device);
    let io = IoBus::new([DeviceConnection {
        device,
        mmio: std::iter::once(0x3000..=0x4fff).collect(),
        ports: std::iter::once(0x3d4..=0x3d4).collect(),
    }]);
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            eip: 0x100,
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x5000,
        map(),
    )
    .unwrap();
    let memory = Memory::new(config.machine).unwrap();
    memory.view().write(0x100, &[0x40, 0xeb, 0]).unwrap(); // INC AX; JMP +0.
    let mut creating = Box::pin(Cpu::new(memory.clone(), config.cpu, io.clone()));
    let mut cpu = match futures_lite::future::poll_once(creating.as_mut()).await {
        Some(cpu) => cpu.unwrap(),
        None => {
            // Compilation may suspend without excluding synchronous host I/O.
            io.write_port(0x3d4, 2, 0x2468);
            assert_eq!(io.read_port(0x3d4, 2), 0x2468);
            creating.as_mut().await.unwrap()
        }
    };
    drop(creating);
    let resources = memory.with_io(io);
    let retained = resources.clone();
    drop(resources);
    {
        let mut view = retained.view();
        view.write_u16(0x200, 0x1234).unwrap();
        view.write_u16(0x3000, 0x5678).unwrap();
        // Neither the view nor the retained bus borrows CPU storage or a device.
        cpu.context().cpu.set_ax(41);
        assert_eq!(
            crate::support::checkpoint(&mut cpu).await,
            ExecutionExit::Interrupted
        );
        assert_eq!(cpu.context().cpu.ax(), 42);
        assert_eq!(cpu.context().memory.read_u16(0x200).unwrap(), 0x1234);
        assert_eq!(cpu.context().memory.read_u16(0x3000).unwrap(), 0x5678);
        retained.io().write_port(0x3d4, 2, 0xabcd);
        assert_eq!(cpu.context().io.read_port(0x3d4, 2), 0xabcd);
        drop(cpu);
        assert_eq!(view.read_u16(0x200).unwrap(), 0x1234);
        view.write_u16(0x3000, 0x9abc).unwrap();
        assert_eq!(view.read_u16(0x3000).unwrap(), 0x9abc);
        assert_eq!(retained.io().read_port(0x3d4, 2), 0xabcd);
    }
    assert!(lifetime.upgrade().is_some());
    drop(retained);
    assert!(lifetime.upgrade().is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn retained_memory_bus_survives_cpu_and_releases_devices() {
    pollster::block_on(check_retained_memory_bus());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn retained_memory_bus_survives_cpu_and_releases_devices() {
    check_retained_memory_bus().await;
}

#[cfg(not(target_arch = "wasm32"))]
async fn cpu(io: IoBus) -> Cpu {
    let config = support::configuration(
        Compiler::new(ExecutionProfile::Real16).target(),
        CpuState {
            segments: Segments::real_mode(),
            ..Default::default()
        },
        0x5000,
        map(),
    )
    .unwrap();
    Cpu::new(Memory::new(config.machine).unwrap(), config.cpu, io)
        .await
        .unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "an I/O bus supports exactly one CPU")]
fn one_bus_cannot_be_attached_to_two_live_cpus() {
    pollster::block_on(async {
        let io = IoBus::default();
        let _first = cpu(io.clone()).await;
        let _second = cpu(io.clone()).await;
    });
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "an I/O bus supports exactly one CPU")]
fn dropping_a_cpu_does_not_make_its_bus_rebindable() {
    pollster::block_on(async {
        let io = IoBus::default();
        drop(cpu(io.clone()).await);
        let _second = cpu(io.clone()).await;
    });
}
