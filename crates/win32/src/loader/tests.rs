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

const MATH: BuiltinDll<u32> = BuiltinDll::new(
    "Math.dll",
    &[
        DllExport::function::<scale>()
            .aliases(&["ScaleAlias"])
            .ordinal(17),
        DllExport::stateless::<private>().ordinal_only(400),
    ],
);

#[test]
fn named_import_and_dynamic_ordinal_share_the_installed_entry() {
    pollster::block_on(async {
        let page = Rc::new(Page([0xa5; 64].into()));
        let mut loader = DllLoader::new(page.clone(), BASE + 8..=BASE + 15, |_| {});
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
                        (BASE + 13, Some(Ordinal(400)))
                    ]
                );
                Ok(())
            })
            .await
            .unwrap();

        let module = loader.module("MATH.DLL").unwrap();
        let ordinal = module.resolve(Ordinal(17)).unwrap();
        assert_eq!(module.resolve(Name("ScaleAlias")), Some(ordinal));
        assert_eq!(module.resolve(Ordinal(400)), Some(BASE + 13));
        assert_eq!(module.resolve(Name("Private")), None);
        assert_eq!(module.resolve(Ordinal(1)), None);

        assert_eq!(
            loader.bind_import("mAtH.dLl", Name("Scale"), BASE),
            Ok(Some(ordinal))
        );
        let mut slot = [0; 4];
        page.read(BASE, &mut slot).unwrap();
        assert_eq!(u32::from_le_bytes(slot), ordinal);
        assert_eq!(slot, [8, 0x10, 0, 0x70]);
        assert_eq!(
            &page.0.borrow()[8..16],
            &[0x90, 0x90, 0xc2, 4, 0, 0x90, 0x90, 0xc3]
        );

        assert_eq!(
            loader.bind_import("Math.dll", Name("scale"), BASE + 4),
            Ok(None)
        );
        assert_eq!(
            loader.bind_import("Other.dll", Ordinal(17), BASE + 4),
            Ok(None)
        );
        assert_eq!(&page.0.borrow()[4..8], &[0xa5; 4]);
        assert!(loader
            .bind_import("Math.dll", Ordinal(17), BASE + 62)
            .is_err());
        assert_eq!(&page.0.borrow()[62..], &[0xa5; 2]);
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
        let mut loader = DllLoader::new(page, BASE..=BASE + 10, |_| {});
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
