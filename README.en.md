# JAI

[中文](README.md) | **English**

A desktop app that wraps the web versions of popular AI assistants: one window, several tabs, sites you configure, downloads in one place.

> Unofficial project. Not affiliated with, endorsed by, or authorized by DeepSeek, Moonshot AI (Kimi), Alibaba (Qwen) or any other site it can open.

## Features

### Several AIs in one window

- Tabbed wrapper: keep DeepSeek, Kimi, Qwen, Doubao … open together, or add any http/https URL
- Up to 9 tabs open at once; the configured list itself can be as long as you like
- Links inside a site stay in that tab; external links go to your system browser

### Sites you configure

- Add, rename, re-point, reorder or delete sites — the list order is the tab order
- Open the first N tabs on launch (N = 0–9); the rest wait in the tab bar's **▾** list
- **Drag to reorder**: drag any tab in the tab bar (configured-but-unopened sites don't take part); the order you leave is written back to your configuration, so the next launch matches what you see
- **Any tab can be closed with its ×**; when only one is left, the × is greyed out — at least one tab stays open

### Downloads all land in one folder

- Every site's downloads go to the folder you pick (the system download folder by default) — change it any time in Settings
- Optional "one subfolder per site"; duplicate names get ` (1)`, ` (2)`
- When a download finishes, a small translucent toast slides out of the tab bar: **✓ 下载完成　立即查看　×**
  - **立即查看** reveals the file in Finder / File Explorer
  - **×** dismisses it; otherwise it disappears after 2.6 seconds

### Closing the window doesn't quit (system tray)

- The window's close button hides it to the system tray instead of quitting, so an answer in progress isn't interrupted
- Tray menu: **显示主窗口** (show main window) / **退出** (quit) — the app only quits from there
- On macOS, clicking the Dock icon brings the window back; on Linux the tray is disabled, so closing quits

### Tab bar controls

**⟳** reload current tab · **＋** add a tab · **▾** open a configured site · **⚙** settings (click the same button again to close the panel)

## Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `Cmd/Ctrl + 1~9` | Switch to tab 1–9 |
| `Esc` | Close the side panel |

On macOS there is also a **标签** (Tabs) menu that mirrors the currently open tabs.

## Using it

1. **First launch** — DeepSeek and Kimi are preconfigured; just sign in (phone number or QR code is recommended)
2. **Add a site** — click **＋** in the tab bar, fill in a name and URL; tick "save to config" to keep it for next time
3. **Change sites** — click **⚙** to rename, re-point, reorder or delete, and to set how many tabs open on launch
4. **Change the download folder** — **⚙** → download folder → **选择…** (choose); **默认** (default) restores the system folder

The configuration file `tabs.json` can also be edited by hand (it takes effect on the next launch):

| Platform | Path |
| --- | --- |
| macOS | `~/Library/Application Support/com.kelvin.jai/tabs.json` |
| Linux | `~/.config/com.kelvin.jai/tabs.json` |
| Windows | `%APPDATA%\com.kelvin.jai\tabs.json` |

## Getting a build

The repository ships no prebuilt binaries. Pushing to `main` (or triggering it manually) runs the `build` workflow, which builds macOS, Windows and Ubuntu in parallel — grab the artifacts from that run.

> The installers are not code-signed: on macOS, right-click → Open the first time; on Windows, accept the SmartScreen prompt ("Run anyway").

## Running locally (developers)

```bash
npm install
npm run tauri dev
```

During development, prefer a debug build (much faster): `npm run tauri build -- --debug --bundles app`.

## Notes and known issues

- **Google sign-in** is refused inside an embedded window (Google's own security policy, not this app's), so use phone number or QR code
- Linux uses WebKitGTK, whose rendering is weaker than macOS/Windows — some pages may look slightly different
- The user interface is currently Chinese only
