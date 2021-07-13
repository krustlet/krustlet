fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=../crates/kubelet/proto/pluginregistration/v1/pluginregistration.proto");
    println!("cargo:rerun-if-changed=../crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto");

    let builder = tonic_build::configure()
        .format(true)
        .build_client(true)
        .build_server(true);

    std::fs::write("./foo.bar", b"hello world")?;

    // Generate CSI plugin and Device Plugin code
    builder.compile(
        &[
            "../crates/kubelet/proto/pluginregistration/v1/pluginregistration.proto",
            "../crates/kubelet/proto/deviceplugin/v1beta1/deviceplugin.proto",
        ],
        &["../crates/kubelet/proto/pluginregistration/v1", "../crates/kubelet/proto/deviceplugin/v1beta1"],
    )?;

    Ok(())
}
