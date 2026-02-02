use std::{env, fs, path::PathBuf};
use zstd::bulk::compress;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let input = manifest_dir.join("data/models.dev.min.json");
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
    let output = out_dir.join("models.dev.min.json.zst");

    let json = fs::read(&input).expect("read models.dev.min.json");
    let compressed = compress(&json, 22).expect("zstd compress");
    fs::write(&output, compressed).expect("write models.dev.min.json.zst");

    println!("cargo:rerun-if-changed={}", input.display());
}
