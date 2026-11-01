//! Windows: woa.ico als Icon-Ressource in die woa.exe einbetten. Die Taskleiste und der
//! Explorer zeigen das Exe-Icon; ohne Ressource steht dort ein Platzhalter. Linux: nichts.

fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/woa.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/windows/woa.ico");
        res.compile().expect("Icon-Ressource für woa.exe");
    }
}
