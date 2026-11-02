//! On Windows, the icon put into the exe, for Explorer and the taskbar.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("FileDescription", "OpenJK-rpmod Updater");
        res.set("ProductName", "OpenJK-rpmod Updater");
        res.compile().expect("the Windows resources couldn't be built");
    }
}
