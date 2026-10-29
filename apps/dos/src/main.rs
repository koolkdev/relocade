fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let result = (|| -> Result<(), String> {
            let mut args = std::env::args_os().skip(1);
            let program = match args.next() {
                Some(path) => std::fs::read(path).map_err(|error| error.to_string())?,
                None => relocade_dos_app::demo_program(),
            };
            if args.next().is_some() {
                return Err("usage: relocade-dos-app [program.com|program.exe]".into());
            }
            let control = relocade_app::open(
                relocade_dos_app::DosApplication::new(program),
                Default::default(),
            )?;
            pollster::block_on(control)?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
