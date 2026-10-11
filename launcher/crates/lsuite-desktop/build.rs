//! Embeds the app icon and version into lsuite.exe on Windows.

fn main() {
    println!("cargo:rerun-if-changed=resources/lsuite.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("resources/lsuite.ico");
        res.set("ProductName", "lsuite");
        res.set("FileDescription", "lsuite — the lsuite apps, their plugins and the lsuite agent");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {e}");
        }
    }
}
