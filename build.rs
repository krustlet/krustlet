fn main() -> Result<(), Box<dyn std::error::Error>> {
    // We need to build the proto files so that we can use them in the tests
    println!("cargo:rerun-if-changed=crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto");

    let builder = tonic_build::configure()
        .format(true)
        .build_client(true)
        .build_server(true);

    // Generate Device Plugin code
    builder.compile(
        &["crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto"],
        &[
            "crates/kubelet/proto/pluginregistration/v1",
            "crates/kubelet/proto/deviceplugin/v1beta1",
        ],
    )?;

    Ok(())
}
