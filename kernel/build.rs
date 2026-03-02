use std::process::Command;
use std::path::PathBuf;

fn main() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();

    let os_disk_dir = project_root.join("os_disk");
    let tar_output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("disk.tar");

    let status = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "cd {} && tar cf {} *",
            os_disk_dir.display(),
            tar_output.display()
        ))
        .status()
        .expect("Failed to run tar command");

    if !status.success() {
        panic!("Failed to create disk.tar");
    }

    println!("cargo:rerun-if-changed={}", os_disk_dir.display());
}
