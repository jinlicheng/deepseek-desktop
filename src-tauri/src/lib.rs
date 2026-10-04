mod config;
mod download;
mod tabs;
mod tray;

use std::sync::Mutex;

use tauri::menu::{MenuBuilder, SubmenuBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, RunEvent, WindowEvent};
use tabs::{Panel, TabMenu, Tabs};
use tauri_plugin_opener::OpenerExt;

/// 与前端 src/styles.css 中 #tabbar 的 height 保持一致
const TABBAR_HEIGHT: f64 = 40.0;
/// 右侧停靠面板的宽度（与前端 .panel 的 width 一致）
const PANEL_WIDTH: f64 = 340.0;
pub(crate) const WIN_LABEL: &str = "main";
const MENU_TAB_PREFIX: &str = "mtab-";

#[derive(Default)]
struct TabGeometry {
    /// 标题栏高度补偿（逻辑像素）= 窗口高度 − 页面视口高度。
    /// 全屏时为 0，macOS 窗口模式约 28。由本地页面上报的数据校准后缓存。
    title_bar: Mutex<f64>,
    /// 最近一次生效的内容区几何。**只能由 apply_bounds 写入**：
    /// 任何绕过它直接改几何的代码都会让这里失真，导致去重逻辑错误地跳过必要的更新。
    last_applied: Mutex<Option<(LogicalPosition<f64>, LogicalSize<f64>)>>,
    /// 当前打开的右侧面板（None / Add / Available / Settings）
    panel: Mutex<Option<Panel>>,
}

pub(crate) fn current_panel(app: &AppHandle) -> Option<Panel> {
    *app.state::<TabGeometry>().panel.lock().unwrap()
}

pub(crate) fn set_panel_str(app: &AppHandle, panel: Option<Panel>) {
    *app.state::<TabGeometry>().panel.lock().unwrap() = panel;
}

/// 子 webview 的位置与尺寸（tab 栏下方；面板打开时让出右侧宽度）。
///
/// macOS 上 `window.inner_size()` 把标题栏高度也算在内，而子 webview 的坐标系基于
/// 包含标题栏的那个视图：不补偿的话子 webview 会整体上移一个标题栏高度（约 28px），
/// 把顶部的标签栏盖住；全屏没有标题栏，所以恰好正常。补偿量由本地页面
/// （`window.innerHeight`，即真实可见内容区高度）校准得出，全屏与 Windows/Linux 上为 0，
/// 不需要任何平台判断。
pub(crate) fn content_bounds(
    app: &AppHandle,
    window: &tauri::Window,
) -> tauri::Result<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let scale = window.scale_factor()?;
    let win = window.inner_size()?.to_logical::<f64>(scale);
    let title_bar = *app.state::<TabGeometry>().title_bar.lock().unwrap();
    let panel = if current_panel(app).is_some() {
        PANEL_WIDTH
    } else {
        0.0
    };
    let content_height = (win.height - title_bar).max(0.0);
    Ok((
        LogicalPosition::new(0.0, title_bar + TABBAR_HEIGHT),
        LogicalSize::new(
            (win.width - panel).max(0.0),
            (content_height - TABBAR_HEIGHT).max(0.0),
        ),
    ))
}

thread_local! {
    /// 同一线程是否正处在 `apply_bounds` 内部：重入探测用，见下面第 2 条说明
    static APPLYING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 几何的唯一入口：计算当前应有的内容区几何，记录到 last_applied 并应用到所有已存在的
/// 子 webview。返回该几何，供 reconcile 创建新 webview 时使用。
///
/// 有两处是为了避开 Windows 上的重入自锁，改回去会复现「加标签页后整个窗口僵死」：
/// 1. **不持锁**调用 `set_position` / `set_size`。Windows 上这些调用可能进消息泵，
///    泵里派发出的窗口消息（如 Resized）会重入到本函数；早先这里是持着 `Tabs` 锁调用的，
///    同一线程再抢同一把非可重入锁就是自锁，UI 线程从此不回来。
/// 2. 重入时**只算几何、不重复应用**。调用方（reconcile）要的是返回值，照给不误；
///    应用几何这件事外层正在做，重入这层再做一遍只会多绕一圈。
pub(crate) fn apply_bounds(app: &AppHandle) -> Option<(LogicalPosition<f64>, LogicalSize<f64>)> {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            APPLYING.with(|applying| applying.set(false));
        }
    }

    let nested = APPLYING.with(|applying| applying.replace(true));
    let _reset = Reset;

    let win = app.get_window(WIN_LABEL)?;
    let bounds = content_bounds(app, &win).ok()?;
    if nested {
        crate::debug_log("[bounds] 重入：只算几何，不再应用");
        return Some(bounds);
    }

    let changed = {
        let geometry = app.state::<TabGeometry>();
        let mut last = geometry.last_applied.lock().unwrap();
        let changed = *last != Some(bounds);
        *last = Some(bounds);
        changed
    };
    if changed {
        // 先把标签名取完再放锁：调用 webview 的时候手里不拿任何锁
        let labels: Vec<String> = match app.try_state::<Mutex<Tabs>>() {
            Some(tabs) => tabs
                .lock()
                .unwrap()
                .open
                .iter()
                .map(|t| tabs::label_of(&t.id))
                .collect(),
            None => Vec::new(),
        };
        for label in labels {
            if let Some(wv) = app.get_webview(&label) {
                let _ = wv.set_position(bounds.0);
                let _ = wv.set_size(bounds.1);
            }
        }
        crate::debug_log(&format!(
            "[bounds] 应用 pos=({}, {}) size=({}, {})",
            bounds.0.x, bounds.0.y, bounds.1.width, bounds.1.height
        ));
    }
    Some(bounds)
}

/// 菜单栏的「标签」子菜单。macOS 上 tauri 会自动创建默认菜单，直接追加；
/// Windows / Linux 没有默认菜单（tauri 的默认菜单是 macOS 专属的），需要自建，
/// 否则 app.menu() 为 None 且快捷键无从注册。
fn setup_menu_bar(app: &tauri::App) -> tauri::Result<()> {
    let tabs_menu = SubmenuBuilder::new(app, "标签").build()?;
    match app.menu() {
        Some(menu) => menu.append(&tabs_menu)?,
        None => {
            app.set_menu(MenuBuilder::new(app).item(&tabs_menu).build()?)?;
        }
    }
    app.manage(TabMenu(tabs_menu));
    Ok(())
}

/// 本地页面（标签栏）上报真实视口尺寸：页面加载完与每次缩放都会调用。
/// 用它校准标题栏补偿量，再重算子 webview 几何。
#[tauri::command]
fn report_viewport(app: AppHandle, inner_h: f64) {
    if let Some(win) = app.get_window(WIN_LABEL) {
        if let (Ok(scale), Ok(size)) = (win.scale_factor(), win.inner_size()) {
            let win_height = size.to_logical::<f64>(scale).height;
            *app.state::<TabGeometry>().title_bar.lock().unwrap() =
                (win_height - inner_h).max(0.0);
        }
    }
    apply_bounds(&app);
}

/// 前端渲染错误回报：Vue 默认会吞掉模板/渲染错误（表现为面板空白），
/// 转到终端打印，便于排查前后端字段不匹配这类问题
#[tauri::command]
fn report_ui_error(message: String) {
    eprintln!("[ui-error] {message}");
}

/// 下载提示条上的「立即查看」：在文件管理器里定位最近一次下载的文件
#[tauri::command]
fn reveal_last_download(app: AppHandle) -> Result<(), String> {
    let path = download::last_download().ok_or("还没有下载记录")?;
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| e.to_string())
}

/// 诊断日志：默认不写（`JAI_DL_LOG=1` 启动时才写 `<系统临时目录>/jai_dl.log`）。
///
/// 下载的触发方式、以及 Windows 上创建 webview 卡在哪一步，都只能靠它定位。
/// 所以路径必须走 `temp_dir()` 而不能写死 `/tmp`：后者在 Windows 上会被解析成
/// 「当前盘符根目录下的 \tmp」，目录不存在就静默什么都不写——正是最需要日志的那台机器上没日志。
pub(crate) fn debug_log(line: &str) {
    use std::io::Write as _;
    use std::sync::OnceLock;
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var("JAI_DL_LOG").is_ok()) {
        return;
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(std::env::temp_dir().join("jai_dl.log"))
    {
        let _ = writeln!(f, "{line}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            report_viewport,
            report_ui_error,
            reveal_last_download,
            tabs::get_state,
            tabs::switch_tab,
            tabs::reload_tab,
            tabs::close_tab,
            tabs::open_available,
            tabs::add_tab,
            tabs::update_tab,
            tabs::delete_tab,
            tabs::move_tab,
            tabs::set_open_order,
            tabs::set_startup_count,
            tabs::set_panel,
            tabs::set_download_dir,
            tabs::set_download_per_site,
        ])
        .on_window_event(|window, event| match event {
            // 关闭按钮 → 隐藏到托盘（Linux 未启用托盘，保持「关闭即退出」的默认行为）
            WindowEvent::CloseRequested { api, .. } => {
                if tray::enabled() && !tray::is_quitting() {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
            // 窗口缩放时同步所有子 webview 的几何
            WindowEvent::Resized(_) => {
                apply_bounds(window.app_handle());
            }
            _ => {}
        })
        .setup(|app| {
            app.manage(TabGeometry::default());

            // 每次启动插一条分隔线，便于分辨日志文件里哪一段属于哪次运行
            crate::debug_log(&format!("===== 启动 pid={} =====", std::process::id()));

            let config_path = app.path().app_config_dir()?.join("tabs.json");
            app.manage(Mutex::new(Tabs::load(config_path)));

            setup_menu_bar(app)?;
            if tray::enabled() {
                tray::setup(app.handle())?;
            }
            tabs::reconcile(app.handle(), false);
            tabs::emit_state(app.handle());

            Ok(())
        })
        .on_menu_event(|app, event| {
            if let Some(tab_id) = event.id().as_ref().strip_prefix(MENU_TAB_PREFIX) {
                tabs::switch_to(app, tab_id);
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| match event {
        // 用户主动退出（Cmd+Q、菜单退出、注销等，code 为 None）→ 隐藏而非退出；
        // 托盘「退出」置位 QUITTING 后放行，程序内调用 exit(0) 的 code 为 Some 也不拦截
        RunEvent::ExitRequested { api, code, .. } => {
            if tray::enabled() && code.is_none() && !tray::is_quitting() {
                api.prevent_exit();
                tray::hide_main_window(app_handle);
            }
        }
        // macOS：点击 Dock 图标 → 唤回窗口
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::show_main_window(app_handle),
        _ => {}
    });
}
