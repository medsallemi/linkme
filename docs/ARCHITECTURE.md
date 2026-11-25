# Architecture

- **React and TypeScript** render the Library, Capture, Search and settings in local WebView2 pages. Vite builds their assets.
- **Tauri and Rust** own Windows shortcuts, tray and window lifecycles, clipboard reads, URL validation, system-browser opening, and privileged storage and network requests. IPC capabilities are scoped to the window and command. Renderer content is constrained by CSP.
- **SQLite** stores links, metadata, categories, groups and preferences in the current Windows profile. Schema changes use versioned migrations. Writes are transactional, and duplicate saves retain the existing row.
- **Windows presentation** uses floating Capture and Search windows, DWM Acrylic where available, solid-material fallbacks and click-through desktop dimming. Link Flight starts after a successful insert. Animation does not control persistence.

## Network boundaries

Public-site previews validate destinations and redirects, restrict local addresses and bound response sizes. The renderer receives plain metadata and validated image data. Google Drive uses Google's Desktop OAuth flow with the system browser, loopback, state, PKCE and the per-file backup scope. Windows DPAPI protects local Google session data. Restore validates and imports rows into the running local schema inside a transaction. Eligible update notifications use public GitHub release metadata, with downloads installed manually. [Privacy and permissions](PRIVACY.md)

## Storage performance

In a recorded Windows release-mode storage benchmark with 10,000 synthetic links, indexed browse returned the first page in **0.258 ms p95** and a broad ranked search took **41.329 ms p95**. Each scenario used six warmups and 60 timed samples against file-backed SQLite. These timings cover storage and Rust result materialization. They exclude IPC, WebView2 rendering, typing debounce and window activation.

Run the opt-in storage benchmark from a Windows checkout with the frontend built:

```powershell
cargo test --release --locked --manifest-path src-tauri/Cargo.toml --lib library_performance_baseline -- --ignored --nocapture
```
