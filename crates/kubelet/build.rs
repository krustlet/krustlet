fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/pluginregistration/v1/pluginregistration.proto");
    println!("cargo:rerun-if-changed=proto/deviceplugin/v1beta1/deviceplugin.proto");

    let builder = tonic_build::configure()
        .format(true)
        .build_client(true)
        .build_server(true);

    // #[cfg(test)]
    // let builder = builder.build_server(true);
    // #[cfg(not(test))]
    // let builder = builder.build_server(false);

    // Generate CSI plugin and Device Plugin code
    builder.compile(
        &[
            "proto/pluginregistration/v1/pluginregistration.proto",
            "proto/deviceplugin/v1beta1/deviceplugin.proto",
        ],
        &["proto/pluginregistration/v1", "proto/deviceplugin/v1beta1"],
    )?;

    Ok(())
}
