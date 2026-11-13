use super::*;
use relocade_machine::{MmioRead, MmioWrite};

struct Registers;
impl Device for Registers {}

async fn check_live_mmio() {
    for compiled in [false, true] {
        let io = IoBus::default();
        // Branch after the load so execution returns to the host exit hook.
        let mut cpu = guest(io.clone(), &[0x66, 0xa1, 0, 0x30, 0xeb, 0], &[0], compiled).await;
        let memory = cpu.memory().with_io(io.clone());
        let view = memory.view();
        let writes = Rc::new(RefCell::new(Vec::new()));
        let recorded = writes.clone();
        let mut binding = None;
        io.connect(Rc::new(RefCell::new(Registers)), |io| {
            binding = Some(io.bind_mmio(
                0x3000..=0x4fff,
                &MmioRead::new(|_, _, _| 0x4433_2211),
                &MmioWrite::new(move |_, address, bytes, value| {
                    recorded.borrow_mut().push((address, bytes, value));
                }),
            ));
        });
        let binding = binding.unwrap();
        assert_eq!(view.read_u32(0x3000).unwrap(), 0x4433_2211);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), 0x4433_2211);

        let own_binding = binding.clone();
        let next = MmioRead::new(|_, _, _| 0x8877_6655);
        let resource = Rc::new(0x4433_2211);
        let first = MmioRead::new(move |context, _, _| {
            own_binding.set_read(context, &next);
            *resource
        });
        for host in [true, false] {
            io.with_context(|context| binding.set_read(context, &first));
            if host {
                let mut values = [0; 12];
                view.read(0x3000, &mut values).unwrap();
                assert_eq!(&values[..8], &0x4433_2211u64.to_le_bytes());
                assert_eq!(&values[8..], &0x8877_6655u32.to_le_bytes());
            } else {
                for expected in [0x4433_2211, 0x8877_6655] {
                    cpu.cpu_state().set_eip(0x100);
                    run(&mut cpu).await;
                    assert_eq!(cpu.cpu_state().eax(), expected);
                }
            }
            assert_eq!(view.read_u32(0x3000).unwrap(), 0x8877_6655);
        }
        io.write_mmio(0x3fff, 4, 0x1_4433_2211);
        assert_eq!(&*writes.borrow(), &[(0x3fff, 4, 0x4433_2211)]);
    }
}

async fn check_split_mmio() {
    for compiled in [false, true] {
        let io = IoBus::default();
        let mut next = None;
        io.connect(Rc::new(RefCell::new(Registers)), |io| {
            next = Some(io.bind_mmio(
                0x4000..=0x4fff,
                &MmioRead::new(|_, _, _| 0),
                &MmioWrite::new(|_, _, _, _| panic!("previous write operation")),
            ));
        });
        let next = next.unwrap();
        let reads = next.clone();
        let written = Rc::new(RefCell::new(Vec::new()));
        let captured = written.clone();
        let after = MmioWrite::new(move |_, address, bytes, value| {
            captured.borrow_mut().push((address, bytes, value));
        });
        let captured = written.clone();
        io.connect(Rc::new(RefCell::new(Registers)), |io| {
            io.bind_mmio(
                0x3000..=0x3fff,
                &MmioRead::new(move |context, address, bytes| {
                    assert_eq!((address, bytes), (0x3fff, 1));
                    reads.set_read(context, &MmioRead::new(|_, _, _| 0x0044_3322));
                    0x11
                }),
                &MmioWrite::new(move |context, address, bytes, value| {
                    next.set_write(context, &after);
                    captured.borrow_mut().push((address, bytes, value));
                }),
            );
        });
        let code = [0x66, 0xa1, 0xff, 0x3f, 0x66, 0xa3, 0xff, 0x3f, 0xeb, 0];
        let mut cpu = guest(io.clone(), &code, &[0], compiled).await;
        let memory = cpu.memory().with_io(io.clone());
        assert_eq!(memory.view().read_u32(0x3fff).unwrap(), 0x4433_2211);
        run(&mut cpu).await;
        assert_eq!(cpu.cpu_state().eax(), 0x4433_2211);
        assert_eq!(
            &*written.borrow(),
            &[(0x3fff, 1, 0x11), (0x4000, 3, 0x0044_3322)]
        );
    }
}

test_backends!(
    late_mmio_bindings_and_replacements_reach_existing_guests_and_views,
    check_live_mmio
);
test_backends!(
    split_mmio_transfers_observe_operations_replaced_by_the_previous_part,
    check_split_mmio
);
