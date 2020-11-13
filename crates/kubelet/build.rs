fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/pluginregistration/v1/pluginregistration.proto");

    let builder = tonic_build::configure()
        .format(true)
        .build_client(true)
        .build_server(true);

    // #[cfg(test)]
    // let builder = builder.build_server(true);
    // #[cfg(not(test))]
    // let builder = builder.build_server(false);

    builder.compile(
        &["proto/pluginregistration/v1/pluginregistration.proto"],
        &["proto/pluginregistration/v1"],
    )?;
    Ok(())
}
