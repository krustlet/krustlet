const EXIT_CODE_BOOTSTRAPPED: i32 = 0;
const EXIT_CODE_NEED_APPROVE: i32 = 1;
const EXIT_CODE_NEED_MANUAL_CLEANUP: i32 = 2;

fn main() {
    let cert_paths: Vec<_> = vec! [
        "/home/ivan/.krustlet/config/krustlet-wasi.crt",
        "/home/ivan/.krustlet/config/krustlet-wasi.key",
        "/home/ivan/.krustlet/config/krustlet-wascc.crt",
        "/home/ivan/.krustlet/config/krustlet-wascc.key",
    ].iter().map(std::path::PathBuf::from).collect();

    let status = all_or_none(cert_paths);

    match status {
        AllOrNone::AllExist => std::process::exit(EXIT_CODE_BOOTSTRAPPED),
        AllOrNone::NoneExist => (),
        AllOrNone::Error => std::process::exit(EXIT_CODE_NEED_MANUAL_CLEANUP),
    };

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
