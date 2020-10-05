fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(false)
        .build_client(true)
        .format(true)
        .compile(
            &["proto/pluginregistration/v1/pluginregistration.proto"],
            &["proto/pluginregistration/v1"],
        )?;
    Ok(())
}
