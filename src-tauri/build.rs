fn main() {
    println!("cargo:rerun-if-changed=../scripts/build-ocr.ps1");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let libs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../.tools/ocr/native/install/lib");
        for name in ["tesseract55", "leptonica-1.87.0"] {
            let path = libs.join(format!("{name}.lib"));
            assert!(path.is_file(), "Missing {}. Run npm run prepare:ocr first.", path.display());
            println!("cargo:rerun-if-changed={}", path.display());
            println!("cargo:rustc-link-lib=static={name}");
        }
        println!("cargo:rustc-link-search=native={}", libs.display());
    } else {
        // Non-Windows development uses the native system library, never a CLI.
        println!("cargo:rustc-link-lib=tesseract");
    }
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(
            tauri_build::AppManifest::new().commands(&[
                "get_session",
                "sign_in",
                "sign_out",
                "refresh_session",
                "get_catalog",
                "refresh_catalog",
                "list_readings",
                "confirm_readings",
                "reject_reading",
                "list_outbox",
                "retry_report",
                "get_capture_status",
                "set_capture_enabled",
                "scan_image",
                "select_crops_folder",
                "get_app_info",
                "get_settings",
                "save_settings",
            ]),
        ),
    ).expect("Tauri build failed")
}
