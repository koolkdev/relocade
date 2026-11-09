use sha2::{Digest, Sha256};
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory");
    let root = Path::new(&manifest).join("../..");
    let mut hash = Sha256::new();
    hash.update(b"relocade-interpreter-cache-v1");
    for path in ["Cargo.toml", "Cargo.lock", "crates/interpreter/build.rs"] {
        fingerprint(&root, Path::new(path), &mut hash)?;
    }
    for name in ["compiler", "x86", "x86-profile", "wasm-bindings", "derive"] {
        fingerprint(
            &root,
            Path::new(&format!("crates/{name}/Cargo.toml")),
            &mut hash,
        )?;
        fingerprint(&root, Path::new(&format!("crates/{name}/src")), &mut hash)?;
    }
    println!(
        "cargo:rustc-env=RELOCADE_INTERPRETER_REVISION={:x}",
        hash.finalize()
    );
    Ok(())
}

fn fingerprint(root: &Path, relative: &Path, hash: &mut Sha256) -> std::io::Result<()> {
    let path = root.join(relative);
    // Watching directories includes newly added or removed generator inputs.
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries = fs::read_dir(&path)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        entries.sort();
        for name in entries {
            fingerprint(root, &relative.join(name), hash)?;
        }
    } else {
        let name = relative.to_string_lossy().replace('\\', "/");
        let bytes = fs::read(&path)?;
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    Ok(())
}
