# JAI

[中文](README.md) | **English**

A desktop app that puts the DeepSeek and Kimi web apps in one window, with two tabs and keyboard shortcuts.

> Unofficial project. Not affiliated with, endorsed by, or supported by DeepSeek or Moonshot AI (Kimi).

## Features

- Two tabs: **DeepSeek** (`chat.deepseek.com`) and **Kimi** (`www.kimi.com`)
- Switch tabs with `Cmd/Ctrl + 1` / `Cmd/Ctrl + 2`, or by clicking the tab bar
- In-site links open in the current tab; external links go to the system browser
- Reload button (⟳) in the top right — a fallback for when the web content process is reclaimed
- Only DeepSeek loads at startup; Kimi loads on first activation

## Requirements

| Platform | Install |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Windows | Visual Studio C++ Build Tools; WebView2 runtime (bundled with Windows 11) |
| Linux | `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf` |

You also need Rust (via rustup) and Node.js 24+.

## Development and build

```bash
npm install
npm run tauri dev     # run in development
npm run tauri build   # build installers
```

Build output:

| Platform | Path |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/macos/JAI.app`, `bundle/dmg/JAI_*.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/JAI_*-setup.exe` |
| Linux | `src-tauri/target/release/bundle/deb/JAI_*.deb` |

## Project layout

```
src/index.html                 Tab bar page (local HTML, talks to Rust over IPC)
src-tauri/src/lib.rs           Core logic: window and child webviews, menu shortcuts,
                               geometry, link routing
src-tauri/tauri.conf.json      Window configuration and app metadata
src-tauri/capabilities/        Permissions for the local page
.github/workflows/build.yml    Three-platform CI build
```

## Implementation notes

The app layers native child webviews on top of a local page: the window's own webview draws the tab bar, while the two sites run in native child webviews placed in a fixed content area. The following pitfalls were hit during development and are handled in the code:

- **Multiple webviews require the `unstable` feature**: `WebviewBuilder`, `Window::add_child` and `Manager::get_webview` only exist behind `tauri = { version = "2", features = ["unstable"] }`.
- **New-window requests must be intercepted**: site entry buttons are usually `target="_blank"`, and WKWebView silently drops such requests (the click appears to do nothing). They are routed through `on_new_window`.
- **The macOS title bar offset**: `window.inner_size()` includes the title bar height, while child webviews are positioned relative to a view that includes it too. Using it directly pushes the child webviews roughly 28px up, covering the tab bar (full screen happens to work because there is no title bar). The code calibrates the offset from the local page's `window.innerHeight`; the compensation is automatically 0 in full screen and on Windows/Linux.
- **Never call `Webview::url()` on an `about:blank` page**: wry 0.55.1 unwraps a nil URL and panics (`wkwebview/mod.rs:1349`). Kimi's placeholder state is tracked by a Rust-side flag instead.
- **Do not create webviews from commands or event handlers on Windows**: the Tauri docs state this deadlocks. Both tab webviews are created during `setup()`.
- **Windows/Linux have no default menu**: Tauri's default menu is macOS-only. The code builds its own menu when `app.menu()` is `None`, otherwise the app panics at startup.

## Known caveats

- Installers are not code signed: on macOS, right-click → Open the first time; on Windows, SmartScreen shows a warning — choose "Run anyway".
- Prefer **phone number or QR code** sign-in: Google sign-in is rejected inside embedded webviews (Google's policy, unrelated to this app).
- Linux uses WebKitGTK, which renders modern sites less reliably than macOS/Windows; minor style differences are possible.

## Continuous integration

Pushing to `main` or manually triggering the `build` workflow builds Windows, macOS and Ubuntu in parallel. Artifacts are available on the workflow run page.

## Editor setup (optional)

- [VS Code](https://code.visualstudio.com/) + [Tauri extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
