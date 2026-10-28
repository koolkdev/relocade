use relocade_dos::{Dos, ProcessStatus};
use relocade_x86::{ExecutionExit, HostExit};
use std::io::{Read, Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    pollster::block_on(async {
        // COM programs start at offset 0100h; the message follows 12 bytes of code.
        let mut program = vec![
            0xba, 0x0c, 0x01, // MOV DX,010Ch: address of the message.
            0xb4, 0x09, // MOV AH,09h: print a '$'-terminated string.
            0xcd, 0x21, // INT 21h.
            0xb8, 0x00, 0x4c, // MOV AX,4C00h: exit with status 0.
            0xcd, 0x21, // INT 21h.
        ];
        program.extend_from_slice(b"Hello, DOS!\r\n$");

        let mut dos = Dos::new().await?;
        dos.launch_com(&program, Default::default())?;
        assert_eq!(
            dos.cpu().run(100).await,
            ExecutionExit::Host(HostExit::Return)
        );
        assert_eq!(
            dos.process().unwrap().status(),
            ProcessStatus::Exited(Ok(0))
        );

        let mut output = Vec::new();
        dos.stdout().read_to_end(&mut output)?;
        std::io::stdout().write_all(&output)?;
        Ok(())
    })
}
