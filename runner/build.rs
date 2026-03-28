use std::path::PathBuf;
use std::process::Command;

fn main() {
    let kernel = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/x86_64-blog_os/debug/blog_os");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let bios_path = out_dir.join("bios.img");

    bootloader::BiosBoot::new(&kernel)
        .create_disk_image(&bios_path)
        .unwrap();

    println!("cargo:rustc-env=BIOS_PATH={}", bios_path.display());
}
