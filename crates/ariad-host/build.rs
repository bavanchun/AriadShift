//! Embeds the build target triple and the trusted engine-pack signing keys.

use std::{env, fmt::Write as _, fs, path::PathBuf};

fn main() {
    let target = env::var("TARGET").expect("cargo sets TARGET for build scripts");
    println!("cargo:rustc-env=ARIAD_HOST_TARGET={target}");

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let keys_dir = manifest_dir.join("data").join("pack-keys");
    println!("cargo:rerun-if-changed={}", keys_dir.display());

    let mut keys = Vec::new();
    if let Ok(entries) = fs::read_dir(&keys_dir) {
        for entry in entries {
            let path = entry.expect("read data/pack-keys entry").path();
            if path.extension().is_some_and(|ext| ext == "pub") && path.is_file() {
                keys.push(path);
            }
        }
    }
    keys.sort();

    let mut generated = String::from("pub(crate) const COMPILED_PACK_KEYS: &[(&str, &str)] = &[\n");
    for path in &keys {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("pack key file names are UTF-8");
        writeln!(
            generated,
            "    ({name:?}, include_str!({:?})),",
            path.display().to_string()
        )
        .expect("write to string");
    }
    generated.push_str("];\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    fs::write(out_dir.join("pack_keys.rs"), generated).expect("write pack_keys.rs");
}
