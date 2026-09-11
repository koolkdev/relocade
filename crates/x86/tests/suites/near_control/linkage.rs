use crate::support::{
    machine::{self, Exit, Image, Step},
    step::{Engine, Event, Observation, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, CpuState};

fn image_after_execution(image: &Image, observation: &Observation) -> Image {
    let Some(Event::Return { snapshot, .. }) = observation.events.last() else {
        panic!("the executed block must return to its host");
    };
    let mut after = Image {
        cpu: CpuState::from_bytes(snapshot.cpu.as_slice().try_into().unwrap()),
        guest: image.guest.clone(),
        machine: image.machine.clone(),
    };
    for &(offset, value) in snapshot.guest.as_ref().unwrap() {
        after.data(offset, &[value]);
    }
    after
}

fn check_call_return_linkage(engine: Engine) {
    for (origin, call, ret, return_address, stack, physical_slot, returned) in [
        (
            0x1000,
            &[0xe8, 0x0b, 0, 0, 0][..],
            &[0xc3][..],
            0x1005,
            0x9000,
            0x8000,
            &[5, 0x10, 0, 0][..],
        ),
        (
            0x1234_1000,
            &[0x66, 0xe8, 0x0c, 0],
            &[0x66, 0xc3],
            0x1004,
            0x9002,
            0x8002,
            &[4, 0x10],
        ),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = origin;
        image.cpu.registers.esp = 0x9004;
        image.cpu.instruction_count = 0xffff_fffe;
        image.map(origin >> 12, 0x3000, false);
        image.data(0x3000, call);
        if origin == 0x1000 {
            image.data(0x3010, ret);
        } else {
            image.map(1, 0x5000, false);
            image.data(0x5010, ret);
        }
        image.map(9, 0x8000, true);
        image.data(0x8000, &[0xa5; 8]);
        let pushed = [(physical_slot, returned)];
        let mut cpu = image.cpu;
        cpu.eip = 0x1010;
        cpu.registers.esp = stack;
        cpu.instruction_count = 0xffff_ffff;
        let first = Step {
            cpu,
            ram: &pushed,
            exit: Exit::Dispatch(0x1010),
        };
        cpu.eip = return_address;
        cpu.registers.esp = 0x9004;
        cpu.instruction_count = 0;
        let last = Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(return_address),
        };
        let steps = [first, last];
        assert_eq!(
            engine.observe(TestModule::interpreter(), &image.input(), 2),
            machine::expected(&image, &steps),
            "interpreted CALL/RET from {origin:08x}",
        );

        let call_block = compile_block_from_bytes(origin, call, u32::MAX).unwrap();
        let called = engine.observe(&TestModule::new(&call_block), &image.input(), 1);
        assert_eq!(called, machine::expected(&image, &steps[..1]));
        let after_call = image_after_execution(&image, &called);
        let ret_block = compile_block_from_bytes(0x1010, ret, u32::MAX).unwrap();
        assert_eq!(
            engine.observe(&TestModule::new(&ret_block), &after_call.input(), 1),
            machine::expected(&after_call, &steps[1..]),
            "compiled CALL/RET from {origin:08x}",
        );
    }
}

#[test]
fn calls_return_through_the_saved_link_in_wasmtime() {
    check_call_return_linkage(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn calls_return_through_the_saved_link_in_v8() {
    check_call_return_linkage(Engine::V8);
}
