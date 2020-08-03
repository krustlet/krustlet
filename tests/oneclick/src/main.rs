enum BootstrapReadiness {
    AlreadyBootstrapped,
    NeedBootstrapAndApprove,
    NeedManualCleanup,
}

const EXIT_CODE_BOOTSTRAPPED: i32 = 0;
const EXIT_CODE_NEED_APPROVE: i32 = 1;
const EXIT_CODE_NEED_MANUAL_CLEANUP: i32 = 2;

fn main() {
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

    let exit_code = match readiness {
        BootstrapReadiness::AlreadyBootstrapped => EXIT_CODE_BOOTSTRAPPED,
        BootstrapReadiness::NeedBootstrapAndApprove => EXIT_CODE_NEED_APPROVE,
        BootstrapReadiness::NeedManualCleanup => EXIT_CODE_NEED_MANUAL_CLEANUP,
    };

    std::process::exit(exit_code);
}

fn prepare_for_bootstrap() -> BootstrapReadiness {
    let home_dir = dirs::home_dir().expect("Can't get home dir"); // TODO: allow override of config dir
    let config_dir = home_dir.join(".krustlet/cnfig");

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
    .map(|f| config_dir.join(f))
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
    return BootstrapReadiness::NeedBootstrapAndApprove;
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
        "windows" => ("powershell.exe", "ps1"),
        "linux" | "macos" => ("bash", "sh"),
        os => Err(anyhow::anyhow!("Unsupported OS {}", os))?,
    };

    let repo_root = std::env!("CARGO_MANIFEST_DIR");

    let bootstrap_script = format!("{}/docs/howto/assets/bootstrap.{}", repo_root, ext);
    let bootstrap_output = std::process::Command::new(shell)
        .arg(bootstrap_script)
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

// fn launch_kubelet(name: &str) -> Something {
//     // run the kubelet as a background process using the
//     // same cmd line as in the justfile
//     //
//     // if approval is needed:
//     //   wait for the magic line
//     //   execute the kubectl certificate approve thingy
//     //   verify that we get the 'continuing' notification
//     //   delete the CSRs so that the next process can reuse the host name
//     //
//     // TODO: if we are NOT approving, how do we know the process is ready?
// }
