//! Embeds Nerion's icon (`packaging/icons/nerion.ico`) and its name in the Windows executables:
//! Explorer, the task bar, Task Manager and the firewall prompt show "Nerion Nativo" with Nerion's
//! icon, the name players know this engine by in Nerion's launcher (Nerion).
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let ico = format!("{dir}/../../packaging/icons/nerion.ico");
    println!("cargo:rerun-if-changed={ico}");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&ico);
        resource.set("FileDescription", "Nerion Nativo");
        resource.set("ProductName", "Nerion Nativo");
        resource.set("CompanyName", "Nerion");
        resource.set("InternalName", "nerion-nativo");
        resource.set("OriginalFilename", "nerion-nativo.exe");
        if let Err(e) = resource.compile() {
            panic!("embedding {ico}: {e}");
        }
    }
}
