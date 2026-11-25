# Build Linkme on Windows

## Requirements

- Windows 11 x64 and the Microsoft Edge WebView2 Evergreen Runtime.
- Node.js 24 and npm.
- Rust with the MSVC toolchain, Visual Studio C++ Build Tools, and a Windows SDK.

From the checkout root:

```powershell
npm.cmd ci
npm.cmd run desktop:dev
```

## Checks

Build the frontend before native checks so Tauri has bundled assets available.

```powershell
npm.cmd run build
npm.cmd test
node --test scripts/release-source.node-test.mjs
cargo fmt --all --check --manifest-path src-tauri/Cargo.toml
cargo clippy --locked --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
```

## Package Windows x64

```powershell
npm.cmd run desktop:package -- local
```

The per-user NSIS installer, standalone test executable, and checksums are staged in `src-tauri/target/release/windows-setup-local/`. The script verifies the checksum of its pinned Tauri installer template. Packaging may need network access for tool or template downloads.

A plain source build works offline for core library features. To enable Google sign-in in an installer, use a **Google Desktop app** OAuth export from a project you control with the Drive API enabled. Provide its absolute path in a build shell. Use `--require-drive-oauth` to make missing or invalid build configuration an error.

```powershell
$env:LINKME_DESKTOP_OAUTH_FILE = 'C:\path\to\google-desktop-client.json'
npm.cmd run desktop:package -- local-drive --require-drive-oauth
Remove-Item Env:LINKME_DESKTOP_OAUTH_FILE
```

The application embeds only the public Desktop client fields. Keep the export, tokens and any confidential credentials outside source control. A build without embedded configuration can read a Desktop export from `%LOCALAPPDATA%\app.linkme.desktop\google-oauth.json` for development. The [installed user flow](DRIVE_BACKUP.md) needs no such file when the installer includes configuration.
