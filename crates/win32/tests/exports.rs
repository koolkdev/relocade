#![cfg(not(target_arch = "wasm32"))]

#[path = "exports/flat32.rs"]
mod flat32;

use flat32::{Engine, Flat32, BASE};
use relocade_win32::{
    win32_export, CallingConvention, Exception, Export, GuestMemory, ReturnValue, Signature, VaList,
};
use relocade_x86::CpuState;
use std::cell::Cell;

#[win32_export(name = "AddWide")]
async fn add_wide(#[context] calls: &Cell<u32>, value: u64, delta: i16) -> u64 {
    calls.set(calls.get() + 1);
    value.wrapping_add_signed(i64::from(delta))
}

#[test]
fn indirect_flat32_call_enters_the_declared_export_and_executes_its_near_return() {
    let mut guest = Flat32::new();
    let caller = BASE + 0x100;
    let iat = BASE + 0x4000;
    let export = BASE + 0x2000;
    let stub = add_wide::SIGNATURE.stub();
    guest.write(iat, &export.to_le_bytes()).unwrap();
    guest.write(export, &stub).unwrap();
    let calls = Cell::new(0);
    assert_eq!(add_wide::NAME, "AddWide");
    assert_eq!(add_wide::SIGNATURE.convention(), CallingConvention::Stdcall);

    // PUSH -2, PUSH 12345678h, PUSH 9, CALL [import slot].
    let mut code = vec![
        0x6a, 0xfe, 0x68, 0x78, 0x56, 0x34, 0x12, 0x6a, 9, 0xff, 0x15,
    ];
    code.extend_from_slice(&iat.to_le_bytes());
    guest.write(caller, &code).unwrap();
    for backend in [Engine::Block, Engine::Interpreter] {
        let mut cpu = CpuState {
            eip: caller,
            ..CpuState::default()
        };
        cpu.registers.esp = BASE + 0xf000;
        guest.run(&mut cpu, code.len(), backend);
        assert_eq!(cpu.eip, export);
        assert_eq!(cpu.registers.esp, BASE + 0xeff0);

        let result =
            pollster::block_on(add_wide::entry(&calls, &guest, cpu.registers.esp)).unwrap();
        result
            .into_registers()
            .publish(|eax| cpu.registers.eax = eax, |edx| cpu.registers.edx = edx);
        cpu.eip = export + 2;
        guest.run(&mut cpu, stub.len() - 2, backend);
        assert_eq!(cpu.eip, caller + code.len() as u32);
        assert_eq!(cpu.registers.esp, BASE + 0xf000);
        assert_eq!((cpu.registers.edx, cpu.registers.eax), (0x1234_5678, 7));
    }
    assert_eq!(calls.get(), 2);

    assert!(pollster::block_on(add_wide::entry(&calls, &guest, BASE - 8)).is_err());
    assert_eq!(calls.get(), 2);
}

#[test]
fn guest_callback_reads_its_frame_and_returns_a_register_pair_to_rust() {
    let mut guest = Flat32::new();
    let signature = Signature::<add_wide, u64>::new(CallingConvention::Cdecl);
    let args = add_wide {
        value: 0x7654_3210_abcd_0000,
        delta: -4,
    };
    let mut cpu = CpuState {
        eip: BASE + 0x800,
        ..CpuState::default()
    };
    cpu.registers.esp = BASE + 0xf000;
    let callback = signature
        .enter_callback(&mut cpu, &guest, BASE + 0x3000, &args)
        .unwrap();
    // MOV EAX,[ESP+4]; MOV EDX,[ESP+8]; ADD EAX,[ESP+12]; RET.
    let code = [
        0x8b, 0x44, 0x24, 4, 0x8b, 0x54, 0x24, 8, 0x03, 0x44, 0x24, 12, 0xc3,
    ];
    guest.write(cpu.eip, &code).unwrap();
    guest.run(&mut cpu, code.len(), Engine::Block);
    assert_eq!(cpu.eip, BASE + 0x800);
    assert_eq!(cpu.registers.esp, BASE + 0xeff4);
    assert_eq!(callback.complete(&mut cpu), 0x7654_3210_abcc_fffc);
    assert_eq!(cpu.registers.esp, BASE + 0xf000);
}

#[win32_export(abi = "cdecl")]
fn total(
    #[memory] memory: &dyn GuestMemory,
    count: u32,
    #[varargs] mut tail: VaList,
) -> Result<i32, Exception> {
    let mut result = 0;
    for _ in 0..count {
        result += tail.read::<i32>(memory)?;
    }
    Ok(result)
}

#[test]
fn variadic_export_decodes_the_tail_after_its_fixed_argument() {
    let guest = Flat32::new();
    let esp = BASE + 0xfff0;
    guest
        .write(esp + 4, &[2, 0, 0, 0, 7, 0, 0, 0, 0xfd, 0xff, 0xff, 0xff])
        .unwrap();
    assert_eq!(pollster::block_on(total::entry(&(), &guest, esp)), Ok(4));
    guest.write(esp + 4, &3u32.to_le_bytes()).unwrap();
    assert_eq!(
        pollster::block_on(total::entry(&(), &guest, esp)),
        Err(Exception::PageFault {
            linear_address: BASE + 0x10000,
            error_code: 4,
        })
    );
}

#[win32_export]
async fn yield_once(original: u32) -> u32 {
    let mut yielded = false;
    std::future::poll_fn(|cx| {
        if yielded {
            return std::task::Poll::Ready(());
        }
        yielded = true;
        cx.waker().wake_by_ref();
        std::task::Poll::Pending
    })
    .await;
    original
}

#[test]
fn suspended_export_keeps_its_own_arguments_during_reentry() {
    use std::{
        future::Future,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    let guest = Flat32::new();
    let esp = BASE + 0xf000;
    guest.write(esp + 4, &17u32.to_le_bytes()).unwrap();
    let mut outer = pin!(yield_once::entry(&(), &guest, esp));
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(outer.as_mut().poll(&mut cx), Poll::Pending);
    guest.write(esp + 4, &23u32.to_le_bytes()).unwrap();
    assert_eq!(
        pollster::block_on(yield_once::entry(&(), &guest, esp)),
        Ok(23)
    );
    assert_eq!(outer.poll(&mut cx), Poll::Ready(Ok(17)));
}
