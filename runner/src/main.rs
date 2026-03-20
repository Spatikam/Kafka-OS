fn main() {
    let bios_path = env!("BIOS_PATH");
    let mut cmd = std::process::Command::new("qemu-system-x86_64");
    cmd.arg("-drive").arg(format!("format=raw,file={bios_path}"));
    cmd.arg("-device").arg("isa-debug-exit,iobase=0xf4,iosize=0x04");
    cmd.arg("-serial").arg("stdio");
    // Show the framebuffer in a graphical window
    cmd.arg("-display").arg("sdl");
    cmd.arg("-d").arg("int,cpu_reset");
    cmd.arg("-D").arg("/tmp/qemu.log");
    // --- NETWORKING ---
    cmd.arg("-netdev").arg("user,id=net0");
    cmd.arg("-device").arg("rtl8139,netdev=net0,mac=52:54:00:12:34:56");
    let mut child = cmd.spawn().unwrap();
    child.wait().unwrap();
}
