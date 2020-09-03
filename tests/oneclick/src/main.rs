use std::io::BufRead;

enum BootstrapReadiness {
    AlreadyBootstrapped,
    NeedBootstrapAndApprove,
    NeedManualCleanup,
}

const EXIT_CODE_TESTS_PASSED: i32 = 0;
const EXIT_CODE_TESTS_FAILED: i32 = 1;
const EXIT_CODE_NEED_MANUAL_CLEANUP: i32 = 2;
const EXIT_CODE_BUILD_FAILED: i32 = 3;

fn main() {
    println!("Ensuring all binaries are built...");

    let build_result = build_workspace();

    match build_result {
        Ok(()) => {
            println!("Build succeeded");
        }
        Err(e) => {
            eprintln!("{}", e);
            eprintln!("Build FAILED");
            std::process::exit(EXIT_CODE_BUILD_FAILED);
        }
    }

    println!("Preparing for bootstrap...");

    let readiness = prepare_for_bootstrap();

    match readiness {
        BootstrapReadiness::AlreadyBootstrapped => {
            println!("Already bootstrapped");
        }
        BootstrapReadiness::NeedBootstrapAndApprove => {
            println!("Bootstrap required");
        }
        BootstrapReadiness::NeedManualCleanup => {
            eprintln!("Bootstrap directory and CSRs need manual clean up");
            std::process::exit(EXIT_CODE_NEED_MANUAL_CLEANUP);
        }
    }

    if matches!(readiness, BootstrapReadiness::NeedBootstrapAndApprove) {
        println!("Running bootstrap script...");
        let bootstrap_result = run_bootstrap();
        match bootstrap_result {
            Ok(()) => {
                println!("Bootstrap script succeeded");
            }
            Err(e) => {
                eprintln!("Running bootstrap script failed: {}", e);
                std::process::exit(EXIT_CODE_NEED_MANUAL_CLEANUP);
            }
        }
    }

    let test_result = run_tests(readiness);

    println!("All complete");

    let exit_code = match test_result {
        Ok(()) => EXIT_CODE_TESTS_PASSED,
        Err(_) => EXIT_CODE_TESTS_FAILED,
    };

    std::process::exit(exit_code);
}

fn config_dir() -> std::path::PathBuf {
    let home_dir = dirs::home_dir().expect("Can't get home dir"); // TODO: allow override of config dir
    home_dir.join(".krustlet/config")
}

fn config_file_path_str(file_name: impl AsRef<std::path::Path>) -> String {
    config_dir().join(file_name).to_str().unwrap().to_owned()
}

fn build_workspace() -> anyhow::Result<()> {
    let build_result = std::process::Command::new("cargo")
        .args(&["build"])
        .output()?;

    if build_result.status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{}",
            String::from_utf8(build_result.stderr).unwrap()
        ))
    }
}

fn prepare_for_bootstrap() -> BootstrapReadiness {
    let host_name = hostname::get()
        .expect("Can't get host name")
        .into_string()
        .expect("Can't get host name");

    let cert_paths: Vec<_> = vec![
        "krustlet-wasi.crt",
        "krustlet-wasi.key",
        "krustlet-wascc.crt",
        "krustlet-wascc.key",
    ]
    .iter()
    .map(|f| config_dir().join(f))
    .collect();

    let status = all_or_none(cert_paths);

    match status {
        AllOrNone::AllExist => {
            return BootstrapReadiness::AlreadyBootstrapped;
        }
        AllOrNone::NoneExist => (),
        AllOrNone::Error => {
            return BootstrapReadiness::NeedManualCleanup;
        }
    };

    // We are not bootstrapped, but there may be existing CSRs around

    // TODO: allow override of host names
    let wasi_host_name = &host_name;
    let wascc_host_name = &host_name;

    let wasi_cert_name = format!("{}-tls", wasi_host_name);
    let wascc_cert_name = format!("{}-tls", wascc_host_name);

    let csr_spawn_deletes: Vec<_> = vec![
        "krustlet-wasi",
        "krustlet-wascc",
        &wasi_cert_name,
        &wascc_cert_name,
    ]
    .iter()
    .map(delete_csr)
    .collect();

    let (csr_deletions, csr_spawn_delete_errors) = csr_spawn_deletes.partition_success();

    if !csr_spawn_delete_errors.is_empty() {
        return BootstrapReadiness::NeedManualCleanup;
    }

    let csr_deletion_results: Vec<_> = csr_deletions
        .into_iter()
        .map(|c| c.wait_with_output())
        .collect();

    let (csr_deletion_outputs, csr_run_deletion_failures) =
        csr_deletion_results.partition_success();

    if !csr_run_deletion_failures.is_empty() {
        return BootstrapReadiness::NeedManualCleanup;
    }

    if csr_deletion_outputs.iter().any(|o| !is_resource_gone(o)) {
        return BootstrapReadiness::NeedManualCleanup;
    }

    // We have now deleted all the local certificate files, and all the CSRs that
    // might get in the way of our re-bootstrapping.  Let the caller know they
    // will need to re-approve once the new CSRs come up.
    BootstrapReadiness::NeedBootstrapAndApprove
}

enum AllOrNone {
    AllExist,
    NoneExist,
    Error,
}

fn all_or_none(files: Vec<std::path::PathBuf>) -> AllOrNone {
    let (exist, missing): (Vec<_>, Vec<_>) = files.iter().partition(|f| f.exists());

    if missing.is_empty() {
        return AllOrNone::AllExist;
    }

    for f in exist {
        if matches!(std::fs::remove_file(f), Err(_)) {
            return AllOrNone::Error;
        }
    }

    AllOrNone::NoneExist
}

fn delete_csr(csr_name: impl AsRef<str>) -> std::io::Result<std::process::Child> {
    std::process::Command::new("kubectl")
        .args(&["delete", "csr", csr_name.as_ref()])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
}

trait ResultSequence {
    type SuccessItem;
    type FailureItem;
    fn partition_success(self) -> (Vec<Self::SuccessItem>, Vec<Self::FailureItem>);
}

impl<T, E: std::fmt::Debug> ResultSequence for Vec<Result<T, E>> {
    type SuccessItem = T;
    type FailureItem = E;
    fn partition_success(self) -> (Vec<Self::SuccessItem>, Vec<Self::FailureItem>) {
        let (success_results, error_results): (Vec<_>, Vec<_>) =
            self.into_iter().partition(|r| r.is_ok());
        let success_values = success_results.into_iter().map(|r| r.unwrap()).collect();
        let error_values = error_results
            .into_iter()
            .map(|r| r.err().unwrap())
            .collect();
        (success_values, error_values)
    }
}

fn is_resource_gone(kubectl_output: &std::process::Output) -> bool {
    kubectl_output.status.success()
        || match String::from_utf8(kubectl_output.stderr.clone()) {
            Ok(s) => s.contains("NotFound"),
            _ => false,
        }
}

fn run_bootstrap() -> anyhow::Result<()> {
    let (shell, ext) = match std::env::consts::OS {
        "windows" => Ok(("powershell.exe", "ps1")),
        "linux" | "macos" => Ok(("bash", "sh")),
        os => Err(anyhow::anyhow!("Unsupported OS {}", os)),
    }?;

    let repo_root = std::env!("CARGO_MANIFEST_DIR");

    let bootstrap_script = format!("{}/docs/howto/assets/bootstrap.{}", repo_root, ext);
    let bootstrap_output = std::process::Command::new(shell)
        .arg(bootstrap_script)
        .env("CONFIG_DIR", config_dir())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()?;

    match bootstrap_output.status.code() {
        Some(0) => Ok(()),
        Some(e) => Err(anyhow::anyhow!(
            "Bootstrap error {}: {}",
            e,
            String::from_utf8_lossy(&bootstrap_output.stderr)
        )),
        None => Err(anyhow::anyhow!(
            "Bootstrap error (no exit code): {}",
            String::from_utf8_lossy(&bootstrap_output.stderr)
        )),
    }
}

fn launch_kubelet(
    name: &str,
    kubeconfig_suffix: &str,
    kubelet_port: i32,
    need_csr: bool,
) -> anyhow::Result<OwnedChildProcess> {
    // run the kubelet as a background process using the
    // same cmd line as in the justfile:
    // KUBECONFIG=$(eval echo $CONFIG_DIR)/kubeconfig-wasi cargo run --bin krustlet-wasi {{FLAGS}} -- --node-name krustlet-wasi --port 3001 --bootstrap-file $(eval echo $CONFIG_DIR)/bootstrap.conf --cert-file $(eval echo $CONFIG_DIR)/krustlet-wasi.crt --private-key-file $(eval echo $CONFIG_DIR)/krustlet-wasi.key
    let bootstrap_conf = config_file_path_str("bootstrap.conf");
    let cert = config_file_path_str(format!("{}.crt", name));
    let private_key = config_file_path_str(format!("{}.key", name));
    let kubeconfig = config_file_path_str(format!("kubeconfig-{}", kubeconfig_suffix));
    let port_arg = format!("{}", kubelet_port);

    let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin_path = repo_root.join("target/debug").join(name);

    let mut launch_kubelet_process = std::process::Command::new(bin_path)
        .args(&[
            "--node-name",
            name,
            "--port",
            &port_arg,
            "--bootstrap-file",
            &bootstrap_conf,
            "--cert-file",
            &cert,
            "--private-key-file",
            &private_key,
            "--x-allow-local-modules",
            "true",
        ])
        .env("KUBECONFIG", kubeconfig)
        .env(
            "RUST_LOG",
            "wascc_host=debug,wascc_provider=debug,wasi_provider=debug,main=debug",
        )
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    println!("Kubelet process {} launched", name);

    if need_csr {
        println!("Waiting for kubelet {} to generate CSR", name);
        let stdout = launch_kubelet_process.stdout.as_mut().unwrap();
        wait_for_tls_certificate_approval(stdout)?;
        println!("Finished bootstrapping for kubelet {}", name);
    }

    let terminator = OwnedChildProcess {
        terminated: false,
        child: launch_kubelet_process,
    };
    Ok(terminator)
}

fn wait_for_tls_certificate_approval(stdout: impl std::io::Read) -> anyhow::Result<()> {
    let reader = std::io::BufReader::new(stdout);
    for (_, line) in reader.lines().enumerate() {
        match line {
            Ok(line_text) => {
                println!("Kubelet printed: {}", line_text);
                if line_text == "BOOTSTRAP: received TLS certificate approval: continuing" {
                    return Ok(());
                }
                let re = regex::Regex::new(r"^BOOTSTRAP: TLS certificate requires manual approval. Run kubectl certificate approve (\S+)$").unwrap();
                match re.captures(&line_text) {
                    None => (),
                    Some(captures) => {
                        let csr_name = &captures[1];
                        approve_csr(csr_name)?
                    }
                }
            }
            Err(e) => eprintln!("Error reading kubelet stdout: {}", e),
        }
    }
    println!("End of kubelet output with no approval");
    Err(anyhow::anyhow!("End of kubelet output with no approval"))
}

fn approve_csr(csr_name: &str) -> anyhow::Result<()> {
    println!("Approving CSR {}", csr_name);
    let approve_process = std::process::Command::new("kubectl")
        .args(&["certificate", "approve", csr_name])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .output()?;
    if !approve_process.status.success() {
        Err(anyhow::anyhow!(
            "Error approving CSR {}: {}",
            csr_name,
            String::from_utf8(approve_process.stderr).unwrap()
        ))
    } else {
        println!("Approved CSR {}", csr_name);
        clean_up_csr(csr_name)
    }
}

fn clean_up_csr(csr_name: &str) -> anyhow::Result<()> {
    println!("Cleaning up approved CSR {}", csr_name);
    let clean_up_process = std::process::Command::new("kubectl")
        .args(&["delete", "csr", csr_name])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .output()?;
    if !clean_up_process.status.success() {
        Err(anyhow::anyhow!(
            "Error cleaning up CSR {}: {}",
            csr_name,
            String::from_utf8(clean_up_process.stderr).unwrap()
        ))
    } else {
        println!("Cleaned up approved CSR {}", csr_name);
        Ok(())
    }
}

struct OwnedChildProcess {
    terminated: bool,
    child: std::process::Child,
}

impl OwnedChildProcess {
    fn terminate(&mut self) -> anyhow::Result<()> {
        match self.child.kill().and_then(|_| self.child.wait()) {
            Ok(_) => {
                self.terminated = true;
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!(
                "Failed to terminate spawned kubelet process: {}",
                e
            )),
        }
    }
}

impl Drop for OwnedChildProcess {
    fn drop(&mut self) {
        if !self.terminated {
            match self.terminate() {
                Ok(()) => (),
                Err(e) => eprintln!("{}", e),
            }
        }
    }
}

fn run_tests(readiness: BootstrapReadiness) -> anyhow::Result<()> {
    let wasi_process_result = launch_kubelet(
        "krustlet-wasi",
        "wasi",
        3001,
        matches!(readiness, BootstrapReadiness::NeedBootstrapAndApprove),
    );
    let wascc_process_result = launch_kubelet(
        "krustlet-wascc",
        "wascc",
        3000,
        matches!(readiness, BootstrapReadiness::NeedBootstrapAndApprove),
    );

    for process in &[&wasi_process_result, &wascc_process_result] {
        match process {
            Err(e) => {
                eprintln!("Error running kubelet process: {}", e);
                return Err(anyhow::anyhow!("Error running kubelet process: {}", e));
            }
            Ok(_) => println!("Running kubelet process"),
        }
    }

    let test_result = run_test_suite();

    let mut wasi_process = wasi_process_result.unwrap();
    let mut wascc_process = wascc_process_result.unwrap();

    if matches!(test_result, Err(_)) {
        // TODO: ideally we shouldn't have to wait for termination before getting logs
        let terminate_result = wasi_process
            .terminate()
            .and_then(|_| wascc_process.terminate());
        match terminate_result {
            Ok(_) => {
                let wasi_log_destination = std::path::PathBuf::from("./krustlet-wasi-e2e");
                capture_kubelet_logs(
                    "krustlet-wasi",
                    &mut wasi_process.child,
                    wasi_log_destination,
                );
                let wascc_log_destination = std::path::PathBuf::from("./krustlet-wascc-e2e");
                capture_kubelet_logs(
                    "krustlet-wascc",
                    &mut wascc_process.child,
                    wascc_log_destination,
                );
            }
            Err(e) => {
                eprintln!("{}", e);
                eprintln!("Can't capture kubelet logs as they didn't terminate");
            }
        }
    }

    test_result
}

fn run_test_suite() -> anyhow::Result<()> {
    println!("Launching integration tests");
    let test_process = std::process::Command::new("cargo")
        .args(&["test", "--test", "integration_tests"])
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()?;
    println!("Integration tests running");
    // TODO: consider streaming progress
    // TODO: capture pod logs: probably requires cooperation from the test
    // process
    let test_process_result = test_process.wait_with_output()?;
    if test_process_result.status.success() {
        println!("Integration tests PASSED");
        Ok(())
    } else {
        let stdout = String::from_utf8(test_process_result.stdout)?;
        eprintln!("{}", stdout);
        let stderr = String::from_utf8(test_process_result.stderr)?;
        eprintln!("{}", stderr);
        eprintln!("Integration tests FAILED");
        Err(anyhow::anyhow!(stderr))
    }
}

fn capture_kubelet_logs(
    kubelet_name: &str,
    kubelet_process: &mut std::process::Child,
    destination: std::path::PathBuf,
) {
    let stdout = kubelet_process.stdout.as_mut().unwrap();
    let stdout_path = destination.with_extension("stdout.txt");
    write_kubelet_log_to_file(kubelet_name, stdout, stdout_path);

    let stderr = kubelet_process.stderr.as_mut().unwrap();
    let stderr_path = destination.with_extension("stderr.txt");
    write_kubelet_log_to_file(kubelet_name, stderr, stderr_path);
}

fn write_kubelet_log_to_file(
    kubelet_name: &str,
    log: &mut impl std::io::Read,
    file_path: std::path::PathBuf,
) {
    let mut file_result = std::fs::File::create(file_path);
    match file_result {
        Ok(ref mut file) => {
            let write_result = std::io::copy(log, file);
            match write_result {
                Ok(_) => (),
                Err(e) => eprintln!("Can't capture {} output: {}", kubelet_name, e),
            }
        }
        Err(e) => {
            eprintln!("Can't capture {} output: {}", kubelet_name, e);
        }
    }
}
