//! Windows resources for `zpadder.exe` and `zpadder-cli.exe`:
//!
//! - the application manifest: run as the invoking user (no UAC prompt), per-monitor DPI
//!   awareness, UTF-8 code page and long-path support;
//! - version information and the icon Explorer, Task Manager and SmartScreen show;
//! - DLL loading restricted to System32 for the DLLs the exe imports, so a DLL planted next to a
//!   portable `zpadder.exe` (say, in Downloads) is never loaded.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/zpadder.ico");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    embed_manifest::embed_manifest(embed_manifest::new_manifest("ZPadder"))
        .expect("unable to embed the application manifest");

    let mut resources = winresource::WindowsResource::new();
    resources
        .set_icon("../../assets/zpadder.ico")
        .set("ProductName", "ZPadder")
        .set("FileDescription", "ZPadder: map game controller input to keyboard and mouse")
        .set("CompanyName", "The ZPadder Contributors")
        .set("LegalCopyright", "The ZPadder Contributors. MIT OR Apache-2.0.");
    resources.compile().expect("unable to embed version information and the icon");

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // LOAD_LIBRARY_SEARCH_SYSTEM32 for the DLLs in the import table. Run-time loads are
        // restricted by `SetDefaultDllDirectories` at start-up.
        println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
    }
}
