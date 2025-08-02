// build.rs
use std::{env, fs};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR")?;
    let proto_root = Path::new(&manifest_dir).join("proto");
    let proto_file = proto_root.join("secret.proto");

    // re-run if the proto file or directory changes
    println!("cargo:rerun-if-changed={}", proto_file.display());
    println!("cargo:rerun-if-changed={}", proto_root.display());

    tonic_build::configure()
        .build_client(true)
        // write the .rs files into src/proto so you can check them in if you like
        .out_dir("src/secrets")
        // descriptor into OUT_DIR (avoids accidental check–in)
        .file_descriptor_set_path(format!("{}/secret_descriptor.bin", proto_root.display()))
        .compile(
            // input .proto
            &[proto_file.to_str().unwrap()],
            // include root
            &[proto_root.to_str().unwrap()],
        )?;

    // Copy files to the out dir
    let binding = env::var("OUT_DIR")?;
    let out_dir = Path::new(&binding);
    let generated = Path::new("src/secrets/secret_service.rs");
    let dest = out_dir.join("secret_service.rs");
    fs::copy(generated, dest)?;
    
    Ok(())
}