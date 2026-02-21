fn main() {
    let bios_path = env!("BIOS_PATH");
    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    cmd.arg("-drive").arg(format!("format=raw,file={bios_path}"));
    cmd.arg("-device").arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-serial").arg("stdio");
    cmd.arg("-d").arg("int,cpu_reset");
    cmd.arg("-D").arg("/tmp/qemu.log");
    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
