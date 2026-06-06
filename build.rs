//! Build script: compiles Slint UI files and (when the "rdp" feature is
//! active) the FreeRDP C shim.

use std::path::Path;

fn main() {
    // --- Slint (always) ---------------------------------------------------
    println!("cargo:rerun-if-changed=ui/");
    slint_build::compile_with_config(
        "ui/app.slint",
        slint_build::CompilerConfiguration::new()
            .with_include_paths(vec![Path::new("ui/").to_path_buf()]),
    )
    .expect("Slint compilation failed");

    // --- Embed Windows icon -----------------------------------------------
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/meatshell.ico");
        res.compile().expect("failed to embed .ico");
    }

    // --- RDP shim (conditional on the "rdp" feature) --------------------
    // The feature is detected via Cargo's built-in CARGO_FEATURE_RDP.
    #[cfg(feature = "rdp")]
    compile_rdp_shim();
}

#[cfg(feature = "rdp")]
fn compile_rdp_shim() {
    println!("cargo:rerun-if-changed=rdp_shim/");

    // Locate FreeRDP using pkg-config, or fall back to manual paths.
    let lib_freerdp = pkg_config::Config::new()
        .atleast_version("3.0.0")
        .probe("freerdp3")
        .expect("freerdp3 not found via pkg-config. Install libfreerdp3-dev (Linux), freerdp (brew on macOS), or vcpkg install freerdp (Windows).");

    let lib_winpr = pkg_config::Config::new()
        .atleast_version("3.0.0")
        .probe("winpr3")
        .expect("winpr3 not found via pkg-config. Install libwinpr3-dev (Linux).");

    // Collect include paths from both pkg-config results.
    let mut includes: Vec<String> = lib_freerdp
        .include_paths
        .iter()
        .chain(lib_winpr.include_paths.iter())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    includes.dedup();

    // Compile the C shim into a static library.
    cc::Build::new()
        .file("rdp_shim/shim.c")
        .include("rdp_shim")
        .includes(&includes)
        .warnings_into_errors(true)
        .compile("rdp_shim");

    // Let the linker find FreeRDP and WinPR libraries.
    for lib in &lib_freerdp.libs {
        println!("cargo:rustc-link-lib={}", lib);
    }
    for lib in &lib_winpr.libs {
        println!("cargo:rustc-link-lib={}", lib);
    }
    for path in &lib_freerdp.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
    for path in &lib_winpr.link_paths {
        println!("cargo:rustc-link-search=native={}", path.display());
    }
}

