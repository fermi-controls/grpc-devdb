use std::{env, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let incl: &[&str] = &[];
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());

    tonic_build::configure()
        .build_client(false)
        .build_server(true)
        .protoc_arg("--experimental_allow_proto3_optional")
        .file_descriptor_set_path(out_dir.join("devdb_descriptor.bin"))
        .compile(&["src/proto/DevDB.proto"], incl)?;
    Ok(())
}
