use crate::{CompiledCode, Cpu, CpuConfig, FarPointer, FunctionRequest, MachineConfig, Memory};
use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{
    abi, Compiler, CompilerConfig, CpuState, EntryAssumptions, ExecutionExit, MemoryBackend,
    PhysicalMapping, PhysicalMemoryMap,
};

async fn call_after_write() {
    let compiler = Compiler::new(CompilerConfig::new(
        EntryAssumptions::REAL_MODE,
        MemoryBackend::PhysicalBus,
    ));
    let map =
        PhysicalMemoryMap::new([(0..=0x1fff, PhysicalMapping::Ram { backing_offset: 0 })]).unwrap();
    let memory = Memory::new(MachineConfig::new(0x2000, map).unwrap()).unwrap();
    let config = CpuConfig::pc(CpuState::real_mode()).unwrap();
    let mut cpu = Cpu::new(memory.clone(), config, Default::default())
        .await
        .unwrap();
    let runtime = cpu.instance.guest.runtime.clone();
    let wasm_compiler = runtime.with_runtime(|wasm| wasm.compiler());
    memory
        .view()
        .write(0x100, &[0xe8, 0xfd, 0x0e, 0xc3])
        .unwrap(); // CALL 1000; RET
    memory
        .view()
        .write(0x1000, &[0xb8, 7, 0, 0xf4, 0xc3])
        .unwrap(); // MOV AX,7; HLT; RET
    for offset in [0x100, 0x1000] {
        let mut reader = memory.code_reader();
        let request = FunctionRequest {
            entry: FarPointer { segment: 0, offset },
            limits: Default::default(),
        };
        let decoded =
            crate::compiler::discover_functions(compiler.config(), [request], &mut reader).unwrap();
        let blocks = compiler.compile_decoded_blocks(decoded).unwrap();
        let source = reader.finish(
            blocks
                .spans
                .iter()
                .map(|&(address, len)| (address.eip(), len)),
        );
        let module = wasm_compiler.compile(&blocks.bytes).await.unwrap();
        CompiledCode::new(module, blocks, source, Vec::new())
            .unwrap()
            .install(&mut cpu)
            .unwrap();
    }

    // Use the installed caller's own slot. The only operation between patching
    // the immediate and entering that slot is the return from writeCode.
    let mut program = Program::new();
    let write = abi::WRITE_CODE.import(&mut program);
    let calls = program.import_table(abi::CALL_TARGETS);
    abi::RUN
        .define(&mut program, move |mut body, ()| {
            write.call(&mut body, (0x1001_u32, 1_u32, 9_u64))?;
            body.tail_call_indirect(calls, 0, &[])
        })
        .unwrap();
    let probe = wasm_compiler
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let run = runtime.with_runtime(|wasm| {
        let caller = cpu
            .instance
            .code
            .installed()
            .covering(0x100)
            .next()
            .unwrap();
        let calls = caller.calls.as_ref().unwrap();
        assert_eq!(calls.targets, [0x1000]);
        let mut imports = cpu.instance.guest.bindings.imports(wasm).to_vec();
        imports.push(cpu.instance.code.write_code.import());
        imports.push(calls.import(wasm));
        let instance = wasm.instantiate(&probe, &imports).unwrap();
        wasm.function(&instance, abi::RUN).unwrap()
    });

    cpu.cpu_state().set_eip(0x1000);
    assert_eq!(memory.installed_code_at(0x1001).len(), 1);
    assert_eq!(
        cpu.instance.enter_guest(|_, wasm| run.call(wasm, ())),
        ExecutionExit::Halted
    );
    assert_eq!(memory.view().read_u8(0x1001).unwrap(), 9);
    assert!(memory.installed_code_at(0x1001).is_empty());
    assert!(cpu.instance.code.installed().dispatch.contains(0x100));
    assert_eq!(cpu.cpu_state().ax(), 9);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn write_code_repairs_a_bound_call_before_returning() {
    pollster::block_on(call_after_write());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn write_code_repairs_a_bound_call_before_returning() {
    call_after_write().await;
}
