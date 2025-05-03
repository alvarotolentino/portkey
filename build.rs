use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=schemas");

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let out_schemas = out_dir.join("schemas");
    println!("cargo:rustc-env=OUT_SCHEMAS={}", out_schemas.display());
    fs::create_dir_all(&out_schemas).unwrap();

    for entry in fs::read_dir("schemas").unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_name = path.file_name().unwrap();
        fs::copy(&path, out_schemas.join(file_name)).unwrap();
    }
}
