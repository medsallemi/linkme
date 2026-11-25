#[path = "src/backup/desktop_client.rs"]
mod desktop_client;

fn configure_desktop_oauth() {
    const INPUT: &str = "LINKME_DESKTOP_OAUTH_FILE";
    println!("cargo:rerun-if-env-changed={INPUT}");
    let bytes = if let Some(input) = std::env::var_os(INPUT) {
        let path = std::path::PathBuf::from(input);
        let name = path
            .to_str()
            .filter(|name| path.is_absolute() && !name.contains(['\r', '\n']))
            .expect("LINKME_DESKTOP_OAUTH_FILE must be an absolute path");
        println!("cargo:rerun-if-changed={name}");
        let client = desktop_client::Client::from_export(&path).expect(
            "LINKME_DESKTOP_OAUTH_FILE must contain a valid Google Desktop app client export",
        );
        // Google Desktop clients are public. Embed only the two fields the token endpoint uses.
        serde_json::to_vec(&client).expect("could not encode Desktop OAuth configuration")
    } else {
        Vec::new()
    };
    let out_dir = std::env::var_os("OUT_DIR").expect("Cargo output directory is required");
    std::fs::write(
        std::path::PathBuf::from(out_dir).join("desktop-oauth.json"),
        bytes,
    )
    .expect("could not write build-time Desktop OAuth configuration");
}

fn main() {
    configure_desktop_oauth();
    let mut attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "reset_linkme",
            "startup_preference",
            "update_preferences",
            "check_for_updates",
            "claim_update_notice",
            "open_update_release",
            "drive_backup",
            "open_drive_privacy_policy",
            "list_drive_backups",
            "restore_drive_backup",
            "preview_preference",
            "preview_link",
            "site_icon",
            "save_link",
            "list_links",
            "list_categories",
            "save_group",
            "order_categories",
            "order_link",
            "update_category",
            "delete_link",
            "copy_link",
            "share_link_email",
            "open_link",
            "connect_capture",
            "update_capture",
            "popup_ready",
            "take_capture_panel",
            "dismiss_capture_panel",
            "save_panel_link",
            "resize_capture_panel",
            "open_search_result",
            "resize_search_panel",
        ]));
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        // Tauri's resource manifest covers app binaries only; the linker also covers unit tests.
        attributes = attributes
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
    tauri_build::try_build(attributes).expect("could not build Linkme native resources");
}
