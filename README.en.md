# JAI

[中文](README.md) | **English**

A desktop app that puts the web versions of AI tools in one window: multiple tabs, with a configurable site list.

> Unofficial project. Not affiliated with, endorsed by, or supported by DeepSeek, Moonshot AI (Kimi) or any other site it can open.

## Features

- **Configurable tab set**: sites live in a config file and can be added, edited, reordered or removed
- **Opens the first N tabs on startup** (N can be 0–9); the rest are one click away in the **▾** list
- **Two kinds of tabs**
  - Pinned: come from the config, have no close button, are removed through the config
  - Ephemeral: opened from **▾** or added without saving, show a **×**, and are disposed when closed
- Four entries in the tab bar: **⟳** reload the active tab, **＋** add a tab, **▾** open a configured site, **⚙** settings (clicking the same button closes the panel again)
- Switch tabs with `Cmd/Ctrl + 1~9` (the **标签** menu is rebuilt to match the open tabs)
- In-site links open in the current tab; external links go to the system browser
- At most 9 tabs open at once (the config list itself is unlimited)

On first run a built-in default config (DeepSeek + Kimi) is used; the config file is written on the first modification.

## Configuration file

`tabs.json` — you can edit it by hand (it is picked up on the next launch):

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/com.kelvin.jai/tabs.json` |
| Linux | `~/.config/com.kelvin.jai/tabs.json` |
| Windows | `%APPDATA%\com.kelvin.jai\tabs.json` |

```json
{
  "startup_count": 2,
  "tabs": [
    { "id": "t_deepseek", "name": "DeepSeek", "url": "https://chat.deepseek.com" },
    { "id": "t_kimi", "name": "Kimi", "url": "https://www.kimi.com" }
  ]
}
```

- `startup_count`: how many of the leading tabs are opened automatically; the rest go to the **▾** list
- `tabs`: the order defines the tab order; `id` only needs to be unique in the file, `url` must be http/https

## Requirements

| Platform | Install |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Windows | Visual Studio C++ Build Tools; WebView2 runtime (bundled with Windows 11) |
| Linux | `libwebkit2gtk-4.1-dev`, `libappindicator3-dev`, `librsvg2-dev`, `patchelf` |

You also need Rust (via rustup) and Node.js 24+. The frontend is Vue 3 + Vite, installed by `npm install`.

## Development and build

```bash
npm install
npm run tauri dev     # dev mode (Vite HMR + debug build)
npm run tauri build   # release build
```

During development prefer a debug build (much faster): `npm run tauri build -- --debug --bundles app`.

Build output:

| Platform | Path |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/macos/JAI.app`, `bundle/dmg/JAI_*.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/JAI_*-setup.exe` |
| Linux | `src-tauri/target/release/bundle/deb/JAI_*.deb` |

## Project layout

```
index.html                          Vite entry
vite.config.js                      Vite config (fixed port 1420, matching devUrl)
src/App.vue                         Root component: subscribes to state-changed, dispatches
                                    commands, switches panels
src/components/TabBar.vue           Tab bar: tab list + ⟳ / ＋ / ▾ / ⚙
src/components/TabDialog.vue        Add-tab form (name / url / save-to-config)
src/components/AvailablePanel.vue   Configured sites that are not currently open
src/components/SettingsPanel.vue    Config management: rename, change url, reorder, delete,
                                    startup count
src-tauri/src/lib.rs                Window and geometry, panel state, bootstrap
src-tauri/src/tabs.rs               Tab state machine, IPC commands, menu rebuild,
                                    webview lifecycle
src-tauri/src/config.rs             Config model and persistence (tabs.json)
src-tauri/tauri.conf.json           Window configuration and app metadata
.github/workflows/build.yml         Three-platform CI build
```

## Implementation notes

The app layers native child webviews on top of a local Vue page: the window's own webview draws the tab bar and panels, while each site runs in a native child webview placed in a fixed content area. The following pitfalls were hit during development and are handled in the code:

- **Multiple webviews require the `unstable` feature**: `WebviewBuilder`, `Window::add_child` and `Manager::get_webview` only exist behind `tauri = { version = "2", features = ["unstable"] }`.
- **The frontend must report `window.innerHeight`** (`report_viewport`): on macOS `window.inner_size()` includes the title bar height, while child webviews are positioned relative to a view that includes it too; without the compensation they shift roughly 28px up and cover the tab bar (full screen happens to work because there is no title bar). The compensation is window height minus page viewport height, and it is automatically 0 in full screen and on Windows/Linux. **Removing that report reintroduces the bug.**
- **Native child webviews always float above the DOM**: panels and forms cannot overlay a web page, so the right-hand panels dock and the webviews give up 340px of width (subtracted in the Rust geometry code).
- **All geometry changes go through `apply_bounds`**, which computes, records (`last_applied`) and applies. Previously `reconcile` changed geometry without updating the record, which made the deduplication skip a needed update and left panels hidden after adding or deleting a tab. Keep new geometry logic on that single path.
- **Do not create webviews from commands or event handlers on Windows** (the Tauri docs state this deadlocks): every create/destroy/recreate is posted to the main thread (`run_on_main_thread`).
- **Windows/Linux have no default menu**: Tauri's default menu is macOS-only. The code builds its own menu when `app.menu()` is `None`, otherwise the app panics at startup.
- **Do not rely on `Webview::url()`**: wry 0.55.1 unwraps a nil URL on `about:blank` pages and panics (`wkwebview/mod.rs:1349`). Reloading uses `Webview::reload()`; placeholder state is tracked in Rust.
- **Vue swallows render errors** (you just see an empty area): `src/main.js` installs an `errorHandler` that reports them to Rust, which prints `[ui-error] ...`. That is how frontend/backend field mismatches surface.

## Known caveats

- Installers are not code signed: on macOS, right-click → Open the first time; on Windows, SmartScreen shows a warning — choose "Run anyway".
- Prefer **phone number or QR code** sign-in: Google sign-in is rejected inside embedded webviews (Google's policy, unrelated to this app).
- Linux uses WebKitGTK, which renders modern sites less reliably than macOS/Windows; minor style differences are possible.

## Continuous integration

Pushing to `main` or manually triggering the `build` workflow builds Windows, macOS and Ubuntu in parallel. Artifacts are available on the workflow run page.

## Editor setup (optional)

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri extension](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
