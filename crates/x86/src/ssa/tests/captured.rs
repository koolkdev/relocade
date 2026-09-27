use super::{Location, StateFields};
use wasm86_compiler::{MemoryImport, Program, Signature, Type, I16, I32, I64, I8};

#[test]
fn captured_records_preserve_held_qwords_and_bytes_outside_narrow_writes() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I64, Type::I64, Type::I64],
    });
    program
        .define(function, |mut body| {
            let index = body.parameter::<I32>(0).unwrap().and(7);
            let base = body.load::<I32>(memory, 0).unwrap();
            let mut record = StateFields::with_base(memory, base);
            let original = record.read(&mut body, Location::<I64>::new(8)).unwrap();
            record
                .define(
                    &mut body,
                    Location::<I64>::new(8),
                    original.xor(0x0102_0304_0506_0708_u64),
                )
                .unwrap();
            let held = record.read(&mut body, Location::<I64>::new(8)).unwrap();
            // The pointer field is outside the captured record. Changing it must not
            // retarget the record or an older value held by its caller.
            body.store::<I32>(memory, 0, 96).unwrap();
            record
                .define(&mut body, Location::<I16>::new(10), 0xabcd)
                .unwrap();
            record
                .define(&mut body, Location::<I8>::indexed(8, 8, index), 0xef)
                .unwrap();
            let current = record.read(&mut body, Location::<I64>::new(8)).unwrap();
            record.publish(&mut body).unwrap();
            body.return_((original, held, current))
        })
        .unwrap();
    program.export("run", function).unwrap();
    let bytes = program.compile().unwrap();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    for base in [32_u32, 80] {
        for (index, result) in [
            (0, 0x8975_6551_abcd_25ef_u64),
            (3, 0x8975_6551_efcd_2519_u64),
            (7, 0xef75_6551_abcd_2519_u64),
        ] {
            let mut store = wasmtime::Store::new(&engine, ());
            let memory =
                wasmtime::Memory::new(&mut store, wasmtime::MemoryType::new(1, None)).unwrap();
            let mut expected: Vec<u8> = (0..256).map(|byte| byte as u8 ^ 0x5a).collect();
            expected[..4].copy_from_slice(&base.to_le_bytes());
            let start = base as usize + 8;
            expected[start..start + 8].copy_from_slice(&0x8877_6655_4433_2211_u64.to_le_bytes());
            memory.write(&mut store, 0, &expected).unwrap();
            let instance = wasmtime::Instance::new(&mut store, &module, &[memory.into()]).unwrap();
            let run = instance
                .get_typed_func::<i32, (i64, i64, i64)>(&mut store, "run")
                .unwrap();
            let values = run.call(&mut store, index).unwrap();
            assert_eq!(
                values,
                (
                    0x8877_6655_4433_2211_u64 as i64,
                    0x8975_6551_4135_2519_u64 as i64,
                    result as i64,
                )
            );
            expected[..4].copy_from_slice(&96_u32.to_le_bytes());
            expected[start..start + 8].copy_from_slice(&result.to_le_bytes());
            assert_eq!(&memory.data(&store)[..256], expected);
        }
    }
}
