use relocade_win32::{
    Arguments, CallingConvention, Exception, GuestMemory, GuestPtr, ReturnRegisters, ReturnValue,
    Signature, StackReader, Win32Args,
};
use relocade_x86::CpuState;
use std::cell::RefCell;

const BASE: u32 = 0x7000_1000;

struct Page(RefCell<[u8; 64]>);

impl Page {
    fn range(
        &self,
        address: u32,
        len: usize,
        write: bool,
    ) -> Result<std::ops::Range<usize>, Exception> {
        let start = address.wrapping_sub(BASE) as usize;
        let end = start
            .checked_add(len)
            .filter(|end| *end <= self.0.borrow().len());
        Ok(start..end.ok_or(Exception::PageFault {
            linear_address: address,
            error_code: if write { 6 } else { 4 },
        })?)
    }
}

impl GuestMemory for Page {
    fn read(&self, address: u32, bytes: &mut [u8]) -> Result<(), Exception> {
        bytes.copy_from_slice(&self.0.borrow()[self.range(address, bytes.len(), false)?]);
        Ok(())
    }

    fn write(&self, address: u32, bytes: &[u8]) -> Result<(), Exception> {
        let range = self.range(address, bytes.len(), true)?;
        self.0.borrow_mut()[range].copy_from_slice(bytes);
        Ok(())
    }
}

#[derive(Win32Args)]
struct Mixed {
    byte: i8,
    wide: u64,
    pointer: GuestPtr<u8>,
}

#[test]
fn callback_frame_uses_guest_slot_widths_and_the_live_continuation() {
    let args = Mixed {
        byte: -7,
        wide: 0x8877_6655_4433_2211,
        pointer: GuestPtr::new(0xfedc_0000),
    };
    for (convention, stub, esp_after_ret) in [
        (
            CallingConvention::Stdcall,
            &[0x90, 0x90, 0xc2, 16, 0][..],
            BASE + 64,
        ),
        (CallingConvention::Cdecl, &[0x90, 0x90, 0xc3][..], BASE + 48),
    ] {
        let signature = Signature::<Mixed, u64>::new(convention);
        let mut cpu = CpuState {
            eip: 0x1234_abcd,
            ..CpuState::default()
        };
        cpu.registers.esp = BASE + 64;
        let page = Page([0xff; 64].into());
        let callback = signature
            .enter_callback(&mut cpu, &page, 0x405000, &args)
            .unwrap();
        assert_eq!(cpu.eip, 0x405000);
        assert_eq!(cpu.registers.esp, BASE + 44);
        assert_eq!(
            &page.0.borrow()[44..],
            &[
                0xcd, 0xab, 0x34, 0x12, 0xf9, 0xff, 0xff, 0xff, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66,
                0x77, 0x88, 0, 0, 0xdc, 0xfe,
            ]
        );
        assert_eq!(signature.stub(), stub);
        cpu.registers.esp = esp_after_ret;
        cpu.registers.edx = 0xdeca_fbad;
        cpu.registers.eax = 0x0123_4567;
        assert_eq!(callback.complete(&mut cpu), 0xdeca_fbad_0123_4567);
        assert_eq!(cpu.registers.esp, BASE + 64);
    }
}

#[test]
fn bad_callback_stack_preserves_cpu_and_memory() {
    let signature = Signature::<Mixed, ()>::new(CallingConvention::Stdcall);
    let mut cpu = CpuState {
        eip: 0x400100,
        ..CpuState::default()
    };
    cpu.registers.esp = BASE + 8;
    let page = Page([0xa5; 64].into());
    let args = Mixed {
        byte: 1,
        wide: 2,
        pointer: GuestPtr::new(3),
    };
    assert_eq!(
        signature
            .enter_callback(&mut cpu, &page, 0x402000, &args)
            .err(),
        Some(Exception::PageFault {
            linear_address: BASE - 12,
            error_code: 6
        }),
    );
    assert_eq!((cpu.eip, cpu.registers.esp), (0x400100, BASE + 8));
    assert_eq!(*page.0.borrow(), [0xa5; 64]);
}

#[test]
fn cdecl_tail_reads_promoted_slots_and_retries_the_faulting_argument() {
    #[derive(Win32Args)]
    struct Fixed(u16);
    let mut page = Page([0; 64].into());
    page.0.get_mut()[44..48].copy_from_slice(&[0x34, 0x12, 0xab, 0xcd]);
    page.0.get_mut()[48..52].copy_from_slice(&(-42i32).to_le_bytes());
    page.0.get_mut()[52..60].copy_from_slice(&1.5f64.to_le_bytes());
    page.0.get_mut()[60..].copy_from_slice(&0xf123_4567u32.to_le_bytes());
    let mut stack = StackReader::at_entry(&page, BASE + 40);
    assert_eq!(Fixed::read(&mut stack).unwrap().0, 0x1234);
    let mut tail = stack.varargs();
    assert_eq!(tail.read::<i32>(&page), Ok(-42));
    assert_eq!(tail.read::<f64>(&page), Ok(1.5));
    assert!(tail.read::<u64>(&page).is_err());
    assert_eq!(
        tail.read::<GuestPtr<u8>>(&page).unwrap().address(),
        0xf123_4567
    );
}

#[test]
fn integer_pointer_and_void_results_select_only_their_registers() {
    assert_eq!(
        (-2i64).into_registers(),
        ReturnRegisters::EdxEax {
            edx: u32::MAX,
            eax: u32::MAX - 1
        }
    );
    let (mut eax, mut edx) = (1, 2);
    for result in [
        GuestPtr::<u8>::new(0xdead_beef).into_registers(),
        ().into_registers(),
    ] {
        result.publish(|value| eax = value, |value| edx = value);
    }
    assert_eq!((eax, edx), (0xdead_beef, 2));
}
