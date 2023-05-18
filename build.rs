fn main() -> Result<(), Box<dyn std::error::Error>> {
    let incl: &[&str] = &[];

    tonic_build::configure()
        .build_client(false)
        .build_server(true)
        .protoc_arg("--experimental_allow_proto3_optional")
        .compile(&["src/proto/DevDB.proto"], incl)?;
    Ok(())
}
