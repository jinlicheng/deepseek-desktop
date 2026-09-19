use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, WebviewUrl};
use tauri_plugin_opener::OpenerExt;

/// 与 src/index.html 中 #tabbar 的 height 保持一致
const TABBAR_HEIGHT: f64 = 40.0;
const WIN_LABEL: &str = "main";
const TAB_DEEPSEEK: &str = "deepseek";
const TAB_KIMI: &str = "kimi";
const MENU_TAB_DEEPSEEK: &str = "menu-tab-deepseek";
const MENU_TAB_KIMI: &str = "menu-tab-kimi";

#[derive(Default)]
struct TabGeometry {
    /// 标题栏高度补偿（逻辑像素）= 窗口高度 − 页面视口高度。
    /// 全屏时为 0，macOS 窗口模式约 28。由本地页面上报的数据校准后缓存。
    title_bar: Mutex<f64>,
    /// 最近一次应用到子 webview 的几何，用于跳过重复设置
    last_applied: Mutex<Option<(LogicalPosition<f64>, LogicalSize<f64>)>>,
    /// Kimi 是否已加载过真实站点（创建时先是 about:blank 占位）
    kimi_loaded: AtomicBool,
}

fn tab_url(name: &str) -> &'static str {
    match name {
        TAB_KIMI => "https://www.kimi.com",
        _ => "https://chat.deepseek.com",
    }
}

/// 子 webview 的位置与尺寸。
///
/// macOS 上 `window.inner_size()` 把标题栏高度也算在内，而子 webview 的坐标系基于
/// 包含标题栏的那个视图：不补偿的话子 webview 会整体上移一个标题栏高度（约 28px），
/// 把顶部的 tab 栏盖住；全屏没有标题栏，所以恰好正常。补偿量由本地页面
/// （`window.innerHeight`，即真实可见内容区高度）校准得出，全屏与 Windows/Linux 上为 0，
/// 不需要任何平台判断。
fn tab_bounds(
    app: &AppHandle,
    window: &tauri::Window,
) -> tauri::Result<(LogicalPosition<f64>, LogicalSize<f64>)> {
    let scale = window.scale_factor()?;
    let win_size = window.inner_size()?.to_logical::<f64>(scale);
    let title_bar = *app.state::<TabGeometry>().title_bar.lock().unwrap();
    let content_height = (win_size.height - title_bar).max(0.0);
    Ok((
        LogicalPosition::new(0.0, title_bar + TABBAR_HEIGHT),
        LogicalSize::new(win_size.width, (content_height - TABBAR_HEIGHT).max(0.0)),
    ))
}

fn apply_tab_bounds(app: &AppHandle) {
    let Some(win) = app.get_window(WIN_LABEL) else {
        return;
    };
    let Ok((position, size)) = tab_bounds(app, &win) else {
        return;
    };
    {
        let geometry = app.state::<TabGeometry>();
        let mut last = geometry.last_applied.lock().unwrap();
        if *last == Some((position, size)) {
            return;
        }
        *last = Some((position, size));
    }
    for name in [TAB_DEEPSEEK, TAB_KIMI] {
        if let Some(wv) = app.get_webview(name) {
            let _ = wv.set_position(position);
            let _ = wv.set_size(size);
        }
    }
}

/// 创建 tab 的 webview 并放到 tab 栏下方。
///
/// 只在 `setup` 阶段调用：Windows 上在同步 command / 事件处理器里创建 webview 会死锁
/// （tauri 文档明确的限制），所以不在切换逻辑里懒创建。
/// Kimi 先用 about:blank 占位，首次切换时才真正加载，避免启动时同时拉两个重型站点。
fn create_tab_webview(app: &AppHandle, name: &str) -> tauri::Result<()> {
    if app.get_webview(name).is_some() {
        return Ok(());
    }
    let win = app.get_window(WIN_LABEL).expect("main window");
    let (position, size) = tab_bounds(app, &win)?;
    let url = if name == TAB_KIMI {
        "about:blank".to_string()
    } else {
        tab_url(name).to_string()
    };

    let handler_app = app.clone();
    let builder = tauri::WebviewBuilder::new(name, WebviewUrl::External(url.parse().unwrap()))
        // target="_blank" / window.open 统一处理：
        // 按域名路由到对应 tab 的 webview，其余外链交给系统浏览器
        .on_new_window(move |url, _features| {
            let host = url.domain().unwrap_or_default().to_lowercase();
            let target = if host.ends_with("deepseek.com") {
                Some(TAB_DEEPSEEK)
            } else if host.ends_with("kimi.com") || host.ends_with("moonshot.cn") {
                Some(TAB_KIMI)
            } else {
                None
            };
            match target.and_then(|tab| handler_app.get_webview(tab)) {
                Some(wv) => {
                    let _ = wv.navigate(url);
                }
                None => {
                    let _ = handler_app.opener().open_url(url.as_str(), None::<&str>);
                }
            }
            tauri::webview::NewWindowResponse::Deny
        });

    let wv = win.add_child(builder, position, size)?;
    // add_child 传入的 bounds 不会应用到原生视图（子 webview 默认是整窗大小），
    // 这里显式设定一次
    let _ = wv.set_position(position);
    let _ = wv.set_size(size);
    if name != TAB_DEEPSEEK {
        let _ = wv.hide();
    }
    Ok(())
}

/// 切换 tab：显隐切换 → 焦点移交给新 tab → 通知 tab 栏更新高亮。
/// 不创建 webview（见 create_tab_webview 的说明），因此可以安全地由命令和菜单事件调用。
fn activate_tab(app: &AppHandle, name: &str) {
    let name = if name == TAB_KIMI { TAB_KIMI } else { TAB_DEEPSEEK };
    let geometry = app.state::<TabGeometry>();
    for tab in [TAB_DEEPSEEK, TAB_KIMI] {
        let Some(wv) = app.get_webview(tab) else {
            continue;
        };
        if tab == name {
            // Kimi 还是占位页时，首次切换才真正加载站点。
            // 注意：这里不能用 webview.url() 判断——wry 0.55.1 在页面为 about:blank 时
            // 会对其返回 nil 的 URL 直接 unwrap 而 panic（wkwebview/mod.rs:1349）。
            let first_activation =
                tab == TAB_KIMI && !geometry.kimi_loaded.swap(true, Ordering::Relaxed);
            if first_activation {
                if let Ok(url) = tab_url(tab).parse() {
                    let _ = wv.navigate(url);
                }
            }
            let _ = wv.show();
            let _ = wv.set_focus();
        } else {
            let _ = wv.hide();
        }
    }
    let _ = app.emit("tab-changed", name);
}

#[tauri::command]
fn switch_tab(app: AppHandle, name: String) {
    activate_tab(&app, &name);
}

/// 刷新指定 tab（webview 页面进程被系统回收后的兜底手段）
#[tauri::command]
fn reload_tab(app: AppHandle, name: String) {
    if let Some(wv) = app.get_webview(name.as_str()) {
        let _ = wv.reload();
    }
}

/// 本地页面（tab 栏）上报真实视口尺寸：页面加载完与每次缩放都会调用。
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
    apply_tab_bounds(&app);
}

/// 菜单栏的「标签」子菜单，含 Cmd/Ctrl+1、Cmd/Ctrl+2 快捷键
fn setup_menu(app: &tauri::App) -> tauri::Result<()> {
    let item_deepseek = MenuItemBuilder::with_id(MENU_TAB_DEEPSEEK, "DeepSeek")
        .accelerator("CmdOrCtrl+1")
        .build(app)?;
    let item_kimi = MenuItemBuilder::with_id(MENU_TAB_KIMI, "Kimi")
        .accelerator("CmdOrCtrl+2")
        .build(app)?;
    let tabs_menu = SubmenuBuilder::new(app, "标签")
        .items(&[&item_deepseek, &item_kimi])
        .build()?;

    match app.menu() {
        // macOS：tauri 会自动创建默认菜单，直接追加即可
        Some(menu) => menu.append(&tabs_menu)?,
        // Windows / Linux：tauri 的默认菜单是 macOS 专属的，这里自建一个
        // （否则 app.menu() 为 None，快捷键也就无从注册）
        None => {
            app.set_menu(MenuBuilder::new(app).item(&tabs_menu).build()?)?;
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            switch_tab,
            reload_tab,
            report_viewport
        ])
        .on_window_event(|window, event| {
            // 窗口缩放时同步两个子 webview（tab 栏高度固定，不随窗口缩放）
            if let tauri::WindowEvent::Resized(_) = event {
                apply_tab_bounds(window.app_handle());
            }
        })
        .setup(|app| {
            app.manage(TabGeometry::default());

            // 两个 tab 的 webview 都在这里创建（见 create_tab_webview 的说明）
            create_tab_webview(app.handle(), TAB_DEEPSEEK)?;
            create_tab_webview(app.handle(), TAB_KIMI)?;
            activate_tab(app.handle(), TAB_DEEPSEEK);

            setup_menu(app)?;
            Ok(())
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_TAB_DEEPSEEK => activate_tab(app, TAB_DEEPSEEK),
            MENU_TAB_KIMI => activate_tab(app, TAB_KIMI),
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
