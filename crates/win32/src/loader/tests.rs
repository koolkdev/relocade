use super::*;
use crate::win32_export;
use DllSymbol::{Name, Ordinal};

#[path = "../../tests/support/page.rs"]
mod page;
use page::{Page, BASE};

#[win32_export(name = "Scale")]
fn scale(#[context] factor: &u32, value: u32) -> u32 {
    value.wrapping_mul(*factor)
}

#[win32_export(name = "Private")]
fn private() {}

#[win32_export(name = "Version")]
fn version() -> u32 {
    1
}

const MATH: BuiltinDll<u32> = BuiltinDll::new(
    "Math.dll",
    &[
        DllExport::function::<scale>()
            .aliases(&["ScaleAlias"])
            .ordinal(17),
        DllExport::stateless::<private>().ordinal_only(400),
        DllExport::stateless::<version>(),
    ],
);

#[test]
fn one_loaded_module_resolves_imports_and_exports_used_only_dynamically() {
    pollster::block_on(async {
        let page = Rc::new(Page([0xa5; 64].into()));
        let mut loader = DllLoader::new(page.clone(), BASE + 8..=BASE + 18, |_| {});
        loader
            .load_with(&MATH, async |functions| {
                let entries: Vec<_> = functions
                    .iter()
                    .map(|(address, export)| (*address, export.symbols().next()))
                    .collect();
                assert_eq!(
                    entries,
                    [
                        (BASE + 8, Some(Name("Scale"))),
                        (BASE + 13, Some(Ordinal(400))),
                        (BASE + 16, Some(Name("Version"))),
                    ]
                );
                Ok(())
            })
            .await
            .unwrap();

        let module = loader.module("MATH.DLL").unwrap();
        let imports = [Name("Scale"), Ordinal(400)];
        let slots = imports.map(|symbol| module.resolve(symbol).unwrap().to_le_bytes());
        page.write(BASE, slots.as_flattened()).unwrap();
        let mut first_slot = [0; 4];
        page.read(BASE, &mut first_slot).unwrap();
        let scale_address = u32::from_le_bytes(first_slot);
        assert_eq!(module.resolve(Ordinal(17)), Some(scale_address));
        assert_eq!(module.resolve(Name("ScaleAlias")), Some(scale_address));
        assert_eq!(module.resolve(Name("Version")), Some(BASE + 16));
        assert_eq!(module.resolve(Name("scale")), None);
        assert_eq!(module.resolve(Name("Private")), None);
        assert_eq!(module.resolve(Ordinal(1)), None);
        assert!(loader.module("Other.dll").is_none());
        assert_eq!(
            &page.0.borrow()[..8],
            &[8, 0x10, 0, 0x70, 13, 0x10, 0, 0x70]
        );
        assert_eq!(
            &page.0.borrow()[8..19],
            &[0x90, 0x90, 0xc2, 4, 0, 0x90, 0x90, 0xc3, 0x90, 0x90, 0xc3]
        );
    });
}

#[test]
fn loaded_dll_addresses_belong_to_the_process_and_survive_repeated_loads() {
    const SAME_NAME: BuiltinDll<()> =
        BuiltinDll::new("mATH.DLL", &[DllExport::function::<private>()]);
    pollster::block_on(async {
        for offset in [0, 32] {
            let page = Rc::new(Page([0; 64].into()));
            let mut loader = DllLoader::new(page, BASE + offset..=BASE + 63, |_| {});
            loader.load_with(&MATH, async |_| Ok(())).await.unwrap();
            let existing = loader
                .load_with(&SAME_NAME, async |_| panic!("reinstalled"))
                .await
                .unwrap();
            assert_eq!(existing.resolve(Name("Scale")), Some(BASE + offset));
        }
    });
}

#[test]
fn duplicate_names_and_ordinals_are_rejected_before_writing_stubs() {
    const BAD_NAME: BuiltinDll<u32> = BuiltinDll::new(
        "bad.dll",
        &[
            DllExport::function::<scale>(),
            DllExport::stateless::<private>().aliases(&["Scale"]),
        ],
    );
    const BAD_ORDINAL: BuiltinDll<u32> = BuiltinDll::new(
        "bad.dll",
        &[
            DllExport::function::<scale>().ordinal(17),
            DllExport::stateless::<private>().ordinal_only(17),
        ],
    );
    pollster::block_on(async {
        let page = Rc::new(Page([0xa5; 64].into()));
        let mut loader = DllLoader::new(page.clone(), BASE..=BASE + 63, |_| {});
        for (dll, duplicate) in [(&BAD_NAME, Name("Scale")), (&BAD_ORDINAL, Ordinal(17))] {
            let result = loader
                .load_with(dll, async |_| panic!("installed invalid DLL"))
                .await;
            assert!(
                matches!(result, Err(DllLoadError::DuplicateSymbol(symbol)) if symbol == duplicate)
            );
            assert!(loader.module("bad.dll").is_none());
        }
        assert_eq!(*page.0.borrow(), [0xa5; 64]);
    });
}

#[test]
fn failed_installation_preserves_published_modules_and_reuses_private_space() {
    const FIRST: BuiltinDll<()> = BuiltinDll::new("first.dll", &[DllExport::function::<private>()]);
    pollster::block_on(async {
        let page = Rc::new(Page([0; 64].into()));
        let mut loader = DllLoader::new(page, BASE..=BASE + 13, |_| {});
        loader.load_with(&FIRST, async |_| Ok(())).await.unwrap();
        let result = loader
            .load_with(&MATH, async |_| {
                Err(relocade_machine::Error::CompilationTarget)
            })
            .await;
        assert!(matches!(result, Err(DllLoadError::Install(_))));
        assert!(loader.module("Math.dll").is_none());
        assert_eq!(
            loader.module("first.dll").unwrap().resolve(Name("Private")),
            Some(BASE)
        );
        let retried = loader.load_with(&MATH, async |_| Ok(())).await.unwrap();
        assert_eq!(retried.resolve(Ordinal(17)), Some(BASE + 3));
    });
}

#[test]
fn insufficient_or_faulting_stub_storage_never_installs_the_dll() {
    pollster::block_on(async {
        let page = Rc::new(Page([0xa5; 64].into()));
        let mut short = DllLoader::new(page.clone(), u32::MAX - 6..=u32::MAX, |_| {});
        let result = short
            .load_with(&MATH, async |_| panic!("wrapped stub address"))
            .await;
        assert!(matches!(result, Err(DllLoadError::StubSpaceExhausted)));
        let mut unmapped = DllLoader::new(page.clone(), BASE + 60..=BASE + 80, |_| {});
        let result = unmapped
            .load_with(&MATH, async |_| panic!("installed across a fault"))
            .await;
        assert!(matches!(
            result,
            Err(DllLoadError::Memory(Exception::PageFault { .. }))
        ));
        assert!(unmapped.module("math.dll").is_none());
        assert_eq!(*page.0.borrow(), [0xa5; 64]);
    });
}
