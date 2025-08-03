// build.rs
use std::path::Path;
use std::{env, fs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let proto_root = Path::new(&manifest_dir).join("proto");
    let proto_file = proto_root.join("secret.proto");

    // re-run if the proto file or directory changes
    println!("cargo:rerun-if-changed={}", proto_file.display());
    println!("cargo:rerun-if-changed={}", proto_root.display());

    let mut config = prost_build::Config::new();
    config.type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]");

    tonic_build::configure()
        .build_server(false)
        .out_dir("src/secrets")
        .file_descriptor_set_path(format!("{}/secret_descriptor.bin", proto_root.display()))
        .compile_with_config(config, &["proto/secret.proto"], &["proto"])?;

    // Copy files to the out dir
    let binding = env::var("OUT_DIR")?;
    let out_dir = Path::new(&binding);
    let generated = Path::new("src/secrets/secret_service.rs");
    let dest = out_dir.join("secret_service.rs");
    fs::copy(generated, dest)?;

    Ok(())
}
