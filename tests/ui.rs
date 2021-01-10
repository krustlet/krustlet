use compiletest_rs;

use std::path::PathBuf;

#[test]
fn compile_test() {
    let mut config = compiletest_rs::Config::default();
    config.mode = compiletest_rs::common::Mode::Ui;
    config.src_base = PathBuf::from("tests/ui");
    config.link_deps();
    config.clean_rmeta();
    compiletest_rs::run_tests(&config);
}
