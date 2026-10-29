fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        if std::env::args().nth(1).as_deref() == Some("dos") {
            let control = relocade_app::open(
                relocade_app_tests::stress::StressDos::default(),
                Default::default(),
            )
            .unwrap();
            assert!(pollster::block_on(control).unwrap().is_none());
            println!("DOS closed");
            return;
        }
        let application = relocade_app_tests::TestImage::default();
        let observations = application.observations.clone();
        let result =
            relocade_app::open(application, Default::default()).and_then(pollster::block_on);
        match result {
            Ok(result) => {
                assert_eq!(
                    observations
                        .dropped
                        .load(std::sync::atomic::Ordering::Acquire),
                    3
                );
                println!("Application completed: {result:?}");
            }
            Err(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
}
