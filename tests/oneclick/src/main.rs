use std::env;
use std::io::BufRead;
use std::path::{Path, PathBuf};

enum BootstrapReadiness {
    AlreadyBootstrapped,
    NeedBootstrapAndApprove,
    NeedManualCleanup,
}

const EXIT_CODE_TESTS_PASSED: i32 = 0;
const EXIT_CODE_TESTS_FAILED: i32 = 1;
const EXIT_CODE_NEED_MANUAL_CLEANUP: i32 = 2;
const EXIT_CODE_BUILD_FAILED: i32 = 3;
const LOG_DIR: &str = "oneclick-logs";
const NODE_NAME: &str = "krustlet-wasi";

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
    match env::var("KRUSTLET_DATA_DIR") {
        Ok(config_dir) => PathBuf::from(config_dir),
        _ => {
            let home_dir = dirs::home_dir().expect("Can't get home dir");
            home_dir.join(".krustlet/config")
        }
    }
}

fn config_file_path_str(file_name: impl AsRef<std::path::Path>) -> String {
    config_dir().join(file_name).to_str().unwrap().to_owned()
}

fn build_workspace() -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new("cargo");
    #[cfg(target_family = "unix")]
    cmd.args(&["build"]);
    #[cfg(target_family = "windows")]
    cmd.args(&[
        "build",
        "--no-default-features",
        "--features",
        "rustls-tls,kubelet/derive",
    ]);
    let build_result = cmd.output()?;

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

    let cert_paths: Vec<_> = vec!["krustlet-wasi.crt", "krustlet-wasi.key"]
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

    let wasi_cert_name = format!("{}-tls", wasi_host_name);

    let csr_spawn_deletes: Vec<_> = vec!["krustlet-wasi", &wasi_cert_name]
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
    #[cfg(target_family = "unix")]
    let (shell, ext) = ("bash", "sh");
    #[cfg(target_family = "windows")]
    let (shell, ext) = ("powershell.exe", "ps1");

    let repo_root = std::env!("CARGO_MANIFEST_DIR");

    let bootstrap_script = format!("{}/scripts/bootstrap.{}", repo_root, ext);
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
    #[cfg(target_family = "unix")]
    let bin_path = repo_root.join("target/debug").join(name);
    #[cfg(target_family = "windows")]
    let bin_path = repo_root
        .join("target/debug")
        .join(name.to_owned() + ".exe");

    let stderr = std::fs::File::create(Path::new(LOG_DIR).join(format!("{}.stderr", name)))?;

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
            "wasi_provider=debug,main=debug,krator::state=debug,kubelet::container::state=debug",
        )
        .stdout(std::process::Stdio::piped())
        .stderr(stderr)
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
        match self.exited() {
            Ok(true) | Err(_) => {
                eprintln!("Krustlet already exited.");
                return Ok(());
            }
            Ok(false) => (),
        }

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

    fn exited(&mut self) -> anyhow::Result<bool> {
        let exit_status = self.child.try_wait()?;
        Ok(exit_status.is_some())
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
    std::fs::create_dir_all(LOG_DIR)?;
    let wasi_process_result = launch_kubelet(
        NODE_NAME,
        "wasi",
        3001,
        matches!(readiness, BootstrapReadiness::NeedBootstrapAndApprove),
    );

    let mut wasi_process = match wasi_process_result {
        Err(e) => {
            eprintln!("Error running kubelet process: {}", e);
            return Err(anyhow::anyhow!("Error running kubelet process: {}", e));
        }
        Ok(process) => {
            println!("Running kubelet process");
            process
        }
    };

    let test_result = run_test_suite(&mut wasi_process);

    if matches!(test_result, Err(_)) {
        warn_if_premature_exit(&mut wasi_process, NODE_NAME);
        // TODO: ideally we shouldn't have to wait for termination before getting logs
        match wasi_process.exited() {
            Ok(true) | Err(_) => (),
            Ok(false) => {
                let terminate_result = wasi_process.terminate();
                match terminate_result {
                    Ok(_) => (),
                    Err(e) => {
                        eprintln!("{}", e);
                        eprintln!("Can't capture kubelet logs as they didn't terminate");
                        anyhow::bail!("Error terminating Krustlet process.");
                    }
                }
            }
        }
        let wasi_log_destination = std::path::PathBuf::from(LOG_DIR);
        capture_kubelet_logs(
            "krustlet-wasi",
            &mut wasi_process.child,
            wasi_log_destination,
        );
    }

    test_result
}

fn warn_if_premature_exit(process: &mut OwnedChildProcess, name: &str) {
    match process.exited() {
        Err(e) => eprintln!(
            "FAILED checking kubelet process {} exit state ({})",
            name, e
        ),
        Ok(false) => eprintln!("WARNING: Kubelet process {} exited prematurely", name),
        _ => (),
    };
}

fn run_test_suite(krustlet_process: &mut OwnedChildProcess) -> anyhow::Result<()> {
    println!("Launching integration tests");
    let stdout = std::fs::File::create(Path::new(LOG_DIR).join("integration_tests.stdout"))?;
    let stderr = std::fs::File::create(Path::new(LOG_DIR).join("integration_tests.stderr"))?;

    let mut cmd = std::process::Command::new("cargo");
    #[cfg(target_family = "unix")]
    cmd.args(&["test", "--test", "integration_tests"]);
    #[cfg(target_family = "windows")]
    cmd.args(&[
        "test",
        "--test",
        "integration_tests",
        "--no-default-features",
        "--features",
        "rustls-tls,kubelet/derive",
    ]);

    let mut test_process = cmd.stderr(stdout).stdout(stderr).spawn()?;
    println!("Integration tests running");
    let start = std::time::Instant::now();
    loop {
        if let Some(result) = test_process.try_wait()? {
            if result.success() {
                println!("Integration tests PASSED");
                return Ok(());
            } else {
                println!("Integration tests FAILED");
                anyhow::bail!("Integration tests FAILED");
            }
        }
        let now = std::time::Instant::now();
        if now.duration_since(start).as_secs() > 600 {
            anyhow::bail!("Integration tests TIMED OUT");
        }
        match krustlet_process.exited() {
            Ok(true) | Err(_) => {
                eprintln!("Detected Krustlet exited.");
                anyhow::bail!("Detected Krustlet exited.")
            }
            Ok(false) => (),
        }
    }
}

fn capture_kubelet_logs(
    kubelet_name: &str,
    kubelet_process: &mut std::process::Child,
    destination: std::path::PathBuf,
) {
    let stdout = kubelet_process.stdout.as_mut().unwrap();
    let stdout_path = destination.join(format!("{}.stdout", kubelet_name));
    write_kubelet_log_to_file(kubelet_name, stdout, stdout_path);
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
