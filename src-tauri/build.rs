use std::path::Path;

/// In dev the exe lives in target/<profile>/ — the linked FFmpeg DLLs have to
/// sit next to it, or the Windows loader will not find them. `ffmpeg.exe` goes
/// with them: the cast prepare step (cast.rs) spawns the CLI, which in the
/// shared build is a ~0.5 MB shim over those same DLLs.
fn copy_ffmpeg_dlls() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = std::env::var("PROFILE").unwrap();
    let src = Path::new(&manifest).join("ffmpeg").join("bin");
    let dst = Path::new(&manifest).join("target").join(&profile);
    if !src.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dst);
    if let Ok(rd) = std::fs::read_dir(&src) {
        for entry in rd.flatten() {
            let p = entry.path();
            let wanted = p.extension().map(|x| x == "dll").unwrap_or(false)
                || p.file_name().map(|n| n == "ffmpeg.exe").unwrap_or(false);
            if wanted {
                if let Some(name) = p.file_name() {
                    let _ = std::fs::copy(&p, dst.join(name));
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=ffmpeg/bin");
}

/// mpv Lua scripts have to sit next to the dev binary (resource_dir in dev).
fn copy_lua_scripts() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = std::env::var("PROFILE").unwrap();
    let src = Path::new(&manifest).join("lua");
    let dst = Path::new(&manifest)
        .join("target")
        .join(&profile)
        .join("lua");
    if !src.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dst);
    if let Ok(rd) = std::fs::read_dir(&src) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().map(|x| x == "lua").unwrap_or(false) {
                if let Some(name) = p.file_name() {
                    let _ = std::fs::copy(&p, dst.join(name));
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=lua");
}

/// macOS: tauri-plugin-libmpv dlopens libmpv-wrapper.dylib, looking next to the
/// exe and in <exe_dir>/lib. In dev the exe lives in target/<profile>/, so the
/// dylibs from src-tauri/lib are laid out in target/<profile>/lib.
/// The wrapper itself dlopens "libmpv.dylib" by short name, so libmpv.dylib has
/// to be in that same src-tauri/lib: dyld tries that directory through the
/// wrapper's @loader_path rpath. DYLD_LIBRARY_PATH does not work here — npm
/// runs through the SIP-protected /bin/sh, which strips every DYLD_*. Details
/// in FINDINGS-macos.md.
#[cfg(target_os = "macos")]
fn copy_wrapper_dylib() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let profile = std::env::var("PROFILE").unwrap();
    let src = Path::new(&manifest).join("lib");
    let dst = Path::new(&manifest)
        .join("target")
        .join(&profile)
        .join("lib");
    if !src.exists() {
        return;
    }
    let _ = std::fs::create_dir_all(&dst);
    if let Ok(rd) = std::fs::read_dir(&src) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().map(|x| x == "dylib").unwrap_or(false) {
                if let Some(name) = p.file_name() {
                    let out = dst.join(name);
                    // ~50 libraries, 80 MB in total — copy only what changed,
                    // or every build wastes seconds for nothing.
                    let src_meta = p.metadata().ok();
                    let same = match (src_meta.as_ref(), out.metadata().ok()) {
                        (Some(s), Some(d)) => {
                            s.len() == d.len()
                                && match (s.modified(), d.modified()) {
                                    (Ok(sm), Ok(dm)) => dm >= sm,
                                    _ => false,
                                }
                        }
                        _ => false,
                    };
                    if same {
                        continue;
                    }
                    // Homebrew dylibs are mode 444: fs::copy preserves the
                    // permissions, and the next overwrite (including tauri-build
                    // copying resources itself) then fails with EACCES.
                    let _ = std::fs::set_permissions(
                        &out,
                        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(
                            0o644,
                        ),
                    );
                    let _ = std::fs::copy(&p, &out);
                    let _ = std::fs::set_permissions(
                        &out,
                        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(
                            0o644,
                        ),
                    );
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=lib");
}

#[cfg(not(target_os = "macos"))]
fn copy_wrapper_dylib() {}

fn main() {
    copy_ffmpeg_dlls();
    copy_lua_scripts();
    copy_wrapper_dylib();
    // The icon is embedded into the exe as a resource during build.rs — without
    // this line cargo never learns that icon.ico changed and leaves the old exe.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    // The OpenSubtitles application key is baked in with `option_env!`, which is
    // read at COMPILE time and does not register a dependency on the variable by
    // itself: without this line, changing the secret and rebuilding into a warm
    // target directory silently keeps the previous key (or none at all).
    println!("cargo:rerun-if-env-changed=FP_OPENSUBTITLES_KEY");
    // On macOS in dev, tauri-codegen embeds the .icns into the binary and uses
    // it as the Dock icon (in release the bundle provides it) — track it too.
    #[cfg(target_os = "macos")]
    println!("cargo:rerun-if-changed=icons/icon.icns");
    // Inside a .app the resources live in Contents/Resources/lib, while
    // tauri-plugin-libmpv looks for libmpv-wrapper.dylib next to the binary
    // (Contents/MacOS) and does not find it. dyld resolves a short dlopen name
    // through the calling image's LC_RPATH as well, so both directories where
    // the library set may live are added: next to the exe in dev
    // (target/<profile>/lib) and in the bundle (Contents/Resources/lib). dyld
    // simply ignores an rpath that does not exist, so both can be declared.
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/lib");
    #[cfg(target_os = "macos")]
    println!("cargo:rustc-link-arg=-Wl,-rpath,@executable_path/../Resources/lib");
    tauri_build::build()
}
