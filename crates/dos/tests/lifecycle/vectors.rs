use super::{common, ExecutionExit, HostExit, ProcessStatus, ServiceError};

#[test]
fn exits_restore_guest_edited_psp_vectors_and_preserve_other_hooks() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        let memory = dos.cpu().memory();
        for (exit, result) in [
            (&[0xc3][..], Ok(0)), // COM near RET reaches PSP's INT 20h.
            (&[0xcd, 0x20][..], Ok(0)),
            (&[0xb8, 7, 0x4c, 0xcd, 0x21][..], Ok(7)),
            (
                &[0xb8, 0, 0x4b, 0xcd, 0x21][..],
                Err(ServiceError::UnsupportedFunction(0x4b)),
            ),
        ] {
            for (vector, pointer) in [
                (0x22, 0x1357_2468),
                (0x23, 0x2345_1357),
                (0x24, 0x3456_4321),
            ] {
                memory.view().write_u32(vector * 4, pointer).unwrap();
            }
            let mut program = vec![
                0xc7, 0x06, 0x0e, 0, 0xef, 0xbe, // Edit the saved INT 23h offset.
                0xba, 0, 6, // DS:DX = PSP:0600.
                0xb8, 0x22, 0x25, 0xcd, 0x21, // Replace the three live vectors.
                0xb8, 0x23, 0x25, 0xcd, 0x21, 0xb8, 0x24, 0x25, 0xcd, 0x21, 0xb8, 0x60, 0x25, 0xcd,
                0x21, // An unrelated hook survives exit.
                0x31, 0xc0, 0x8e, 0xd8, // DS=0; termination uses the process PSP.
            ];
            program.extend_from_slice(exit);
            dos.launch_com(&program, Default::default()).unwrap();
            let psp = dos.process().unwrap().psp_segment();
            assert_eq!(dos.cpu().run().await, ExecutionExit::Host(HostExit::Return));
            assert_eq!(
                dos.process().unwrap().status(),
                ProcessStatus::Exited(result)
            );
            assert_eq!(memory.view().read_u32(0x22 * 4).unwrap(), 0x1357_2468);
            assert_eq!(memory.view().read_u32(0x23 * 4).unwrap(), 0x2345_beef);
            assert_eq!(memory.view().read_u32(0x24 * 4).unwrap(), 0x3456_4321);
            assert_eq!(
                memory.view().read_u32(0x60 * 4).unwrap(),
                u32::from(psp) << 16 | 0x600
            );
        }
    });
}
