use relocade_compiler::{
    abi::{Function, Memory},
    Program, I32, I64,
};
use relocade_machine::MachineConfig;
use relocade_runtime::{
    wasm::{WasmBackend, WasmContext, WasmMemoryAccess},
    with_host_call, HostCall, RuntimeState,
};
use relocade_x86::{
    Compiler, CpuState, ExecutionContext, ExecutionProfile, PhysicalMapping, PhysicalMemoryMap,
    Registers, Segments,
};
use std::{mem::offset_of, rc::Rc};

const CONTEXT: Memory = Memory::new("host", "context", 1);
const GUEST: Memory = Memory::new("host", "guest", 1);
const CALL: Function<fn() -> I32> = Function::new("host", "call");
const RUN: Function<fn() -> I64> = Function::new("host", "run");

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

struct HostState {
    config: Rc<MachineConfig>,
    calls: u32,
}

// The same handler accesses the calling CPU and guest memory on both backends.
fn host_handler(call: &mut HostCall<'_, HostState>) {
    let mut bytes = [0; 4];
    call.memory().read(0x1020, &mut bytes).unwrap();
    let result = call.cpu().eax() + u32::from_le_bytes(bytes);
    call.cpu().set_eax(result);
    let eip = call.cpu().eip();
    call.cpu().set_eip(eip + 2);
    call.memory().write(0x1024, &result.to_le_bytes()).unwrap();
    call.memory().write(0x20, &[0xff]).unwrap(); // ROM ignores guest writes.
    call.state_mut().calls += 1;
}

async fn check_callback() {
    let model =
        relocade_x86::CpuModel::default().with_feature(relocade_x86::CpuFeature::Cmov, false);
    let mut expected_cpu = CpuState {
        registers: Registers {
            eax: 40,
            ..Registers::default()
        },
        eip: 0x100,
        instruction_count: 7,
        segments: Segments::real_mode(),
        ..CpuState::default()
    };
    let config = MachineConfig::new(
        Compiler::new(ExecutionProfile::Real16)
            .with_cpu_model(model)
            .target(),
        expected_cpu,
        0x3000,
        PhysicalMemoryMap::new([
            (0..=0xfff, PhysicalMapping::Rom { backing_offset: 0 }),
            (
                0x1000..=0x1fff,
                PhysicalMapping::Ram {
                    backing_offset: 0x2000,
                },
            ),
        ])
        .unwrap(),
    )
    .unwrap();
    let state = HostState {
        config: Rc::new(config),
        calls: 0,
    };
    #[cfg(not(target_arch = "wasm32"))]
    let mut backend = Backend::with_state(RuntimeState::new(state)).unwrap();
    #[cfg(target_arch = "wasm32")]
    let mut backend = Backend::with_state(RuntimeState::new(state));
    // Creation order differs from module import order: handles belong to the backend.
    let guest = backend.create_memory(GUEST).unwrap();
    let context = backend
        .create_memory(Memory {
            maximum: Some(1),
            ..CONTEXT
        })
        .unwrap();
    backend.write_memory(
        &context,
        0,
        &ExecutionContext {
            cpu: expected_cpu,
            instruction_target: 998,
            cpu_model: model,
        }
        .to_bytes(),
    );
    backend.write_memory(&guest, 0x2020, &10_u32.to_le_bytes());
    backend.write_memory(&guest, 0x20, &[0xa5]);

    let mut program = Program::new();
    let cpu_memory = program.import_memory(CONTEXT);
    let guest_memory = program.import_memory(GUEST);
    let host_call = program.import_function(CALL);
    let eax_offset = (offset_of!(ExecutionContext, cpu)
        + offset_of!(CpuState, registers)
        + offset_of!(Registers, eax)) as u32;
    let run = program
        .function(RUN.signature(), |mut body| {
            // Publish guest changes before the callback, then reload its changes.
            let eax = body.load::<I32>(cpu_memory, eax_offset)?.add(1);
            body.store(cpu_memory, eax_offset, eax)?;
            let value = body.load::<I32>(guest_memory, 0x2020)?.add(1);
            body.store(guest_memory, 0x2020, value)?;
            body.call::<I32>(host_call, &[])?;
            let register = body
                .load::<I32>(cpu_memory, eax_offset)?
                .unsigned()
                .extend::<I64>();
            let memory = body
                .load::<I32>(guest_memory, 0x2024)?
                .unsigned()
                .extend::<I64>();
            body.return_(register.or(memory.shl(32)))
        })
        .unwrap();
    RUN.export(&mut program, run).unwrap();
    let module = backend.compile(&program.compile().unwrap()).await.unwrap();
    let callback_context = context.clone();
    let callback_guest = guest.clone();
    let imports = [
        backend.bind(CONTEXT, &context),
        backend.bind(GUEST, &guest),
        backend.bind_host(
            CALL,
            Box::new(move |mut caller| {
                let config = Rc::clone(&caller.state().data.config);
                with_host_call(
                    &config,
                    &mut caller,
                    callback_context.clone(),
                    callback_guest.clone(),
                    host_handler,
                );
                0
            }),
        ),
    ];
    let instance = backend.instantiate(&module, &imports).await.unwrap();
    let function = backend.function(&instance, RUN).unwrap();
    for (calls, value, eip) in [(1, 52, 0x102), (2, 65, 0x104)] {
        assert_eq!(
            backend.call(&function).unwrap(),
            i64::from(value) | (i64::from(value) << 32)
        );
        assert_eq!(backend.state().data.calls, calls);
        let mut bytes = [0; ExecutionContext::BYTE_LEN];
        backend.read_memory(&context, 0, &mut bytes);
        expected_cpu.registers.eax = value as u32;
        expected_cpu.eip = eip;
        assert_eq!(
            bytes,
            ExecutionContext {
                cpu: expected_cpu,
                instruction_target: 998,
                cpu_model: model
            }
            .to_bytes()
        );
        let mut byte = [0];
        backend.read_memory(&guest, 0x20, &mut byte);
        assert_eq!(byte, [0xa5]);
    }

    // The same live CPU/memory/state view is available without entering Wasm.
    {
        let mut access = backend.context();
        let config = Rc::clone(&access.state().data.config);
        with_host_call(
            &config,
            &mut access,
            context.clone(),
            guest.clone(),
            host_handler,
        );
    }
    assert_eq!(backend.state().data.calls, 3);
    let mut bytes = [0; ExecutionContext::BYTE_LEN];
    backend.read_memory(&context, 0, &mut bytes);
    expected_cpu.registers.eax = 77;
    expected_cpu.eip = 0x106;
    assert_eq!(
        bytes,
        ExecutionContext {
            cpu: expected_cpu,
            instruction_target: 998,
            cpu_model: model,
        }
        .to_bytes()
    );
    let mut value = [0; 4];
    backend.read_memory(&guest, 0x2024, &mut value);
    assert_eq!(u32::from_le_bytes(value), 77);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_edit_live_cpu_and_mapped_memory() {
    pollster::block_on(check_callback());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_edit_live_cpu_and_mapped_memory() {
    check_callback().await;
}
