use relocade_dos::Dos;
use relocade_machine::ExecutionWorker;

#[test]
fn speaker_ports_work_without_output_and_survive_process_launches() {
    let result = pollster::block_on(ExecutionWorker::run_until(
        std::future::pending::<()>(),
        async |runner| {
            let mut dos = Dos::new(runner).await.unwrap();
            dos.launch_com(
                &[
                    0xb0, 0xb6, 0xe6, 0x43, // Channel 2, low/high bytes, mode 3.
                    0xb0, 0xa9, 0xe6, 0x42, // Divisor 1193, low byte.
                    0xb0, 0x04, 0xe6, 0x42, // High byte.
                    0xb0, 2, 0xe6, 0x61, // Speaker enabled; gate stays low.
                    0xb8, 7, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            assert_eq!(dos.run().await.unwrap(), 7);

            // Read the previous process's programming through actual guest IN/OUT.
            dos.launch_com(
                &[
                    0xe4, 0x61, 0xa2, 0, 4, // Save speaker control and OUT2.
                    0xb0, 0xe8, 0xe6, 0x43, // Channel 2 status latch.
                    0xe4, 0x42, 0x24, 0x3f, 0xa2, 1, 4, // Save access/mode bits.
                    0xb0, 0, 0xe6, 0x61, // Disable the speaker.
                    0xe4, 0x61, 0xa2, 2, 4, 0xb8, 9, 0x4c, 0xcd, 0x21,
                ],
                Default::default(),
            )
            .unwrap();
            assert_eq!(dos.run().await.unwrap(), 9);
            let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
            let mut readback = [0; 3];
            dos.cpu()
                .context()
                .memory
                .read(base + 0x400, &mut readback)
                .unwrap();
            assert_eq!(readback, [0x22, 0x36, 0x20]);
        },
    ));
    assert_eq!(result.unwrap(), Some(()));
}
