# JAI

**中文** | [English](README.en.md)

把 AI 工具的网页版装进一个桌面应用：一个窗口、多个标签、可配置的站点列表。

> 非官方项目，与 DeepSeek、月之暗面（Kimi）等站点均无关联，也未获得其授权或认可。

## 功能

- **可配置的标签集合**：站点保存在配置文件里，可增删改、调序
- **启动时自动打开前 N 个**标签（N 可设为 0~9），其余收在「▾」列表里按需打开
- **两种标签身份**
  - 常驻标签：来自配置，没有关闭按钮，只能删除配置
  - 临时标签：从「▾」打开或添加时未勾选保存，带 **×**，关闭即释放
- 标签栏右侧三个入口：**⟳** 刷新当前标签、**＋** 添加标签、**▾** 打开已配置站点、**⚙** 设置（点击同一按钮可关闭面板）
- `Cmd/Ctrl + 1~9` 切换标签（菜单栏「标签」会随当前标签动态生成）
- 站内链接在当前标签内打开，站外链接交给系统浏览器
- 同时打开的标签上限 9 个（配置列表本身不限长度）

首次运行时使用内置默认配置（DeepSeek + Kimi）；做任意一次修改后才会写入配置文件。

## 配置文件

`tabs.json`，可直接手动编辑（应用下次启动生效）：

| 平台 | 路径 |
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

- `startup_count`：启动时自动打开前几个，超出部分进入「▾」列表
- `tabs`：顺序即标签顺序；`id` 只需在文件内唯一，`url` 需为 http/https

## 环境要求

| 平台 | 需要安装 |
| --- | --- |
| macOS | Xcode Command Line Tools |
| Windows | Visual Studio C++ 生成工具；WebView2 运行时（Windows 11 自带） |
| Linux | `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`librsvg2-dev`、`patchelf` |

另外需要 Rust（通过 rustup 安装）和 Node.js 24+；前端为 Vue 3 + Vite，`npm install` 会一并安装。

## 开发与构建

```bash
npm install
npm run tauri dev     # 开发调试（Vite 热更新 + debug 构建）
npm run tauri build   # 打包 release
```

开发期间建议只做 debug 构建（快得多）：`npm run tauri build -- --debug --bundles app`。

构建产物：

| 平台 | 路径 |
| --- | --- |
| macOS | `src-tauri/target/release/bundle/macos/JAI.app`、`bundle/dmg/JAI_*.dmg` |
| Windows | `src-tauri/target/release/bundle/nsis/JAI_*-setup.exe` |
| Linux | `src-tauri/target/release/bundle/deb/JAI_*.deb` |

## 项目结构

```
index.html                         Vite 入口
vite.config.js                     Vite 配置（固定 1420 端口，配合 devUrl）
src/App.vue                        根组件：订阅 state-changed、分发命令、切换面板
src/components/TabBar.vue          标签栏：标签列表 + ⟳ / ＋ / ▾ / ⚙
src/components/TabDialog.vue       添加标签表单（名称 / 网址 / 是否保存到配置）
src/components/AvailablePanel.vue  已配置但未打开的站点列表
src/components/SettingsPanel.vue   配置管理：改名、改网址、调序、删除、启动个数
src-tauri/src/lib.rs               窗口与几何、面板状态、启动接线
src-tauri/src/tabs.rs              标签状态机、IPC 命令、菜单重建、webview 生命周期
src-tauri/src/config.rs            配置模型与读写（tabs.json）
src-tauri/tauri.conf.json          窗口配置与应用元信息
.github/workflows/build.yml        三平台 CI 构建
```

## 实现要点

整体结构是「原生子 webview 叠在本地 Vue 页面上」：窗口自身的 webview 负责画标签栏与面板，各站点跑在按固定区域摆放的原生子 webview 里。以下是开发中踩到、并已在代码里处理的坑：

- **多 webview 需要开启 `unstable` feature**：`WebviewBuilder`、`Window::add_child`、`Manager::get_webview` 都在 `tauri = { version = "2", features = ["unstable"] }` 之后才可用。
- **前端必须上报 `window.innerHeight`**（`report_viewport`）：macOS 的 `window.inner_size()` 把标题栏高度也算进窗口高度，而子 webview 的坐标系基于包含标题栏的视图；不补偿会让子 webview 整体上移约 28px、盖住标签栏（全屏时因没有标题栏而恰好正常）。补偿量 = 窗口高度 − 页面视口高度，全屏与 Windows/Linux 上自动为 0。**删掉这个上报就会复现该问题。**
- **原生子 webview 永远浮在 DOM 之上**：面板/表单无法覆盖在网页上，因此右侧面板改为「让出 340px 宽度」的停靠式布局（Rust 侧几何计算里减去面板宽度）。
- **几何变更只有一个入口 `apply_bounds`**：它负责计算 + 记录 `last_applied` + 应用。曾经因为 `reconcile` 直接改几何却没更新记录，导致去重逻辑误判、新增/删除标签后面板打不开。新增几何相关逻辑时务必走这个入口。
- **Windows 上不要在命令或事件处理器里创建 webview**（tauri 文档说明会死锁）：所有创建/销毁/重建都投递到主线程（`run_on_main_thread`）后执行。
- **Windows/Linux 没有默认菜单**：tauri 的默认菜单是 macOS 专属的；代码在 `app.menu()` 为 `None` 时自建菜单，否则启动即 panic。
- **不要依赖 `Webview::url()`**：wry 0.55.1 在 `about:blank` 页面上会对返回 nil 的 URL 直接 `unwrap()` 并 panic（`wkwebview/mod.rs:1349`）。刷新用 `Webview::reload()`，占位状态用 Rust 侧数据判断。
- **Vue 会吞掉渲染错误**（表现为区域空白）：`src/main.js` 注册了 `errorHandler`，把前端错误回报给 Rust 打印成 `[ui-error] ...`，便于发现前后端字段不匹配这类问题。

## 已知事项

- 安装包未做代码签名：macOS 首次打开需右键 → 打开；Windows 会弹出 SmartScreen 提示，选择「仍要运行」即可。
- 登录建议使用**手机号或扫码**：Google 登录在嵌入式 webview 中会被 Google 拒绝（对方的安全策略，与本应用无关）。
- Linux 使用 WebKitGTK，渲染兼容性弱于 macOS/Windows，个别页面可能有样式差异。

## 持续集成

推送到 `main` 分支或手动触发 `build` 工作流，会并行构建 Windows、macOS、Ubuntu 三个平台，产物在该次运行的 Artifacts 中下载。

## 开发环境（可选）

- [VS Code](https://code.visualstudio.com/) + [Vue - Official](https://marketplace.visualstudio.com/items?itemName=Vue.volar) + [Tauri 扩展](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
