# JAI

**中文** | [English](README.en.md)

把 DeepSeek 和 Kimi 的网页版装进一个桌面应用：一个窗口、两个标签，快捷键切换。

> 非官方项目，与 DeepSeek、月之暗面（Kimi）均无关联，也未获得其授权或认可。

## 功能

- 两个标签页：**DeepSeek**（`chat.deepseek.com`）和 **Kimi**（`www.kimi.com`）
- `Cmd/Ctrl + 1`、`Cmd/Ctrl + 2` 切换标签，也可以点击顶部标签栏
- 站内链接在当前标签内打开，站外链接交给系统浏览器
- 右上角 ⟳ 刷新当前标签（页面进程被系统回收时的兜底手段）
- 启动只加载 DeepSeek，Kimi 在首次切换时才加载

## 环境要求

| 平台 | 需要安装 |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Windows | Visual Studio C++ 生成工具；WebView2 运行时（Windows 11 自带） |
| Linux | `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`librsvg2-dev`、`patchelf` |

另外需要 Rust（通过 rustup 安装）和 Node.js 24+。

## 开发与构建

```bash
npm install
npm run tauri dev     # 开发调试
npm run tauri build   # 打包
```

构建产物：

| 平台 | 路径 |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/macos/JAI.app`、`bundle/dmg/JAI_*.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/JAI_*-setup.exe` |
| Linux | `src-tauri/target/release/bundle/deb/JAI_*.deb` |

## 项目结构

```
src/index.html                 标签栏页面（本地 HTML，通过 IPC 调用 Rust）
src-tauri/src/lib.rs           核心逻辑：窗口与子 webview、菜单快捷键、几何计算、链接路由
src-tauri/tauri.conf.json      窗口配置与应用元信息
src-tauri/capabilities/        本地页面的权限声明
.github/workflows/build.yml    三平台 CI 构建
```

## 实现要点

这个应用是「原生子 webview 叠在本地页面上」的结构：窗口自己的 webview 负责画标签栏，两个站点分别跑在按固定区域摆放的原生子 webview 里。以下是开发中踩到并已在代码里处理的坑：

- **多 webview 需要开启 `unstable` feature**：`WebviewBuilder`、`Window::add_child`、`Manager::get_webview` 都在 `tauri = { version = "2", features = ["unstable"] }` 之后才可用。
- **必须拦截新窗口请求**：站点入口按钮多为 `target="_blank"`，WKWebView 默认会静默丢弃这类请求（表现为点击无反应），需要 `on_new_window` 自行路由。
- **macOS 的标题栏偏移**：`window.inner_size()` 把标题栏高度也算进窗口高度，而子 webview 的坐标系基于包含标题栏的视图；直接计算会让子 webview 整体上移约 28px 盖住标签栏（全屏时因没有标题栏而恰好正常）。代码用本地页面上报的 `window.innerHeight` 校准，全屏与 Windows/Linux 上该补偿自动为 0。
- **不要对 `about:blank` 页面调用 `Webview::url()`**：wry 0.55.1 会对返回 nil 的 URL 直接 `unwrap()` 并 panic（`wkwebview/mod.rs:1349`），因此 Kimi 的占位状态由 Rust 侧标志位记录。
- **Windows 上不要在命令或事件处理器里创建 webview**：tauri 文档说明会死锁；两个标签的 webview 都在 `setup()` 阶段创建。
- **Windows/Linux 没有默认菜单**：tauri 的默认菜单是 macOS 专属的，代码在 `app.menu()` 为 `None` 时自建菜单，否则启动即 panic。

## 已知事项

- 安装包未做代码签名：macOS 首次打开需右键 → 打开；Windows 会弹出 SmartScreen 提示，选择「仍要运行」即可。
- 登录建议使用**手机号或扫码**：Google 登录在嵌入式 webview 中会被 Google 拒绝（对方的安全策略，与本应用无关）。
- Linux 使用 WebKitGTK，渲染兼容性弱于 macOS/Windows，个别页面可能有样式差异。

## 持续集成

推送到 `main` 分支或手动触发 `build` 工作流，会并行构建 Windows、macOS、Ubuntu 三个平台，产物在该次运行的 Artifacts 中下载。

## 开发环境（可选）

- [VS Code](https://code.visualstudio.com/) + [Tauri 扩展](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
