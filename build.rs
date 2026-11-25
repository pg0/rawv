//! Puts the icon and the version information into the Windows executables, where Explorer,
//! the taskbar and "Open with" read them.

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=assets/rawv.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/rawv.ico")
        .set("FileDescription", "rawv")
        .set("ProductName", "rawv");
    resource.compile()
}
