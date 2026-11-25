// Per platform: what the skathi crate cannot add to an executable it does not own, and our own
// native pieces.
fn main() {
    // Skathi reports the linker arguments its executable needs (macOS: -ObjC, the Swift rpath, weak
    // JSC symbols; Windows: the 8 MiB stack, the ICF rule).
    for arg in std::env::var("DEP_SKATHI_LINK_ARGS").unwrap_or_default().split_whitespace() {
        println!("cargo:rustc-link-arg={arg}");
    }
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos") => macos(),
        Ok("windows") => windows(),
        _ => {}
    }
}

fn macos() {
    for fw in ["OpenGL", "IOSurface", "Metal", "AppKit", "QuartzCore", "CoreGraphics", "CoreFoundation"] {
        println!("cargo:rustc-link-lib=framework={fw}");
    }
    // A development build finds the thumbnail decoder's FFmpeg (version 1's dylibs, through
    // FFMPEG_DIR) by an rpath to the directory the links point into. An installed copy will carry
    // its own rpath to Contents/Frameworks (not built yet).
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    if let Ok(dir) = std::env::var("FFMPEG_DIR") {
        let link = std::path::Path::new(&dir).join("lib/libavutil.dylib");
        if let Some(lib) = std::fs::canonicalize(&link).ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib.display());
        }
    }
    // Fullscreen's one path and its custom transition (fullscreen.rs).
    println!("cargo:rerun-if-changed=src/macos/fullscreen.m");
    cc::Build::new().file("src/macos/fullscreen.m").flag("-fobjc-arc").flag("-Wno-unused-parameter").compile("fp_fullscreen");
    // Quitting stops playback first (quit.rs).
    println!("cargo:rerun-if-changed=src/macos/quit.m");
    cc::Build::new().file("src/macos/quit.m").flag("-fobjc-arc").flag("-Wno-unused-parameter").compile("fp_quit");
    // The overlay's un-premultiply pass (Metal compute between mpv's release and Skathi's copy).
    println!("cargo:rerun-if-changed=src/macos/unpremul.m");
    cc::Build::new()
        .file("src/macos/unpremul.m")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .compile("fp_unpremul");
    // The same kernels precompiled (`kSource` in unpremul.m, unescaped), so the pass need not compile
    // at start (FPUI_METALLIB=1). An empty file when the Metal toolchain is missing.
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let objc = std::fs::read_to_string("src/macos/unpremul.m").unwrap();
    let body = objc.split("kSource =").nth(1).and_then(|t| t.split(";\n").next()).unwrap_or("");
    let mut msl = String::new();
    for line in body.lines() {
        let t = line.trim().trim_start_matches('@');
        if let (Some(a), Some(b)) = (t.find('"'), t.rfind('"')) {
            if b > a {
                msl.push_str(&t[a + 1..b].replace("\\n", "\n").replace("\\\"", "\""));
            }
        }
    }
    std::fs::write(out.join("unpremul.metal"), &msl).unwrap();
    let air = out.join("unpremul.air");
    let lib = out.join("unpremul.metallib");
    let ok = std::process::Command::new("xcrun")
        .args(["-sdk", "macosx", "metal", "-c", "-o"])
        .arg(&air)
        .arg(out.join("unpremul.metal"))
        .status()
        .is_ok_and(|s| s.success())
        && std::process::Command::new("xcrun")
            .args(["-sdk", "macosx", "metallib", "-o"])
            .arg(&lib)
            .arg(&air)
            .status()
            .is_ok_and(|s| s.success());
    if !ok {
        println!("cargo:warning=unpremul.metallib not built; the pass compiles at start");
        std::fs::write(&lib, b"").unwrap();
    }
}

fn windows() {
    // The manifest with Common Controls 6, without which Windows refuses to start the exe.
    if let Ok(dir) = std::env::var("DEP_SKATHI_WINDOWS_RESOURCES") {
        let rc = std::path::Path::new(&dir).join("gpui.rc");
        embed_resource::compile(rc, embed_resource::ParamsIncludeDirs([&dir]))
            .manifest_required()
            .unwrap();
    }
    // The exe's icon: version 1's icon.ico, so nothing binary is kept twice.
    println!("cargo:rerun-if-changed=src/windows/app.rc");
    println!("cargo:rerun-if-changed=../src-tauri/icons/icon.ico");
    embed_resource::compile("src/windows/app.rc", embed_resource::ParamsIncludeDirs(["../src-tauri/icons"]))
        .manifest_optional()
        .unwrap();
}
