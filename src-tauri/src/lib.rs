use std::sync::Mutex;

use tauri::menu::{MenuItemBuilder, SubmenuBuilder};
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

/// 懒加载：首次切换到某个 tab 时才创建它的 webview。
/// 只能在主线程调用（macOS 上 WKWebView 的创建限制）；
/// 同步 command 和菜单事件都运行在主线程，满足要求。
fn ensure_tab_webview(app: &AppHandle, name: &str) -> tauri::Result<()> {
    if app.get_webview(name).is_some() {
        return Ok(());
    }
    let win = app.get_window(WIN_LABEL).expect("main window");
    let (position, size) = tab_bounds(app, &win)?;

    let handler_app = app.clone();
    let builder = tauri::WebviewBuilder::new(name, WebviewUrl::External(tab_url(name).parse().unwrap()))
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
    Ok(())
}

/// 切换 tab：懒创建 → 显隐切换 → 焦点移交给新 tab → 通知 tab 栏更新高亮
fn activate_tab(app: &AppHandle, name: &str) {
    let name = if name == TAB_KIMI { TAB_KIMI } else { TAB_DEEPSEEK };
    let _ = ensure_tab_webview(app, name);
    for tab in [TAB_DEEPSEEK, TAB_KIMI] {
        if let Some(wv) = app.get_webview(tab) {
            if tab == name {
                let _ = wv.show();
                let _ = wv.set_focus();
            } else {
                let _ = wv.hide();
            }
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
        if let Ok(url) = wv.url() {
            let _ = wv.navigate(url);
        }
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

            // DeepSeek 随启动加载；Kimi 首次切换时再创建
            activate_tab(app.handle(), TAB_DEEPSEEK);

            // Cmd+1 / Cmd+2 切换 tab（Windows / Linux 上自动映射为 Ctrl+数字）
            let item_deepseek = MenuItemBuilder::with_id(MENU_TAB_DEEPSEEK, "DeepSeek")
                .accelerator("CmdOrCtrl+1")
                .build(app)?;
            let item_kimi = MenuItemBuilder::with_id(MENU_TAB_KIMI, "Kimi")
                .accelerator("CmdOrCtrl+2")
                .build(app)?;
            let tabs_menu = SubmenuBuilder::new(app, "标签")
                .items(&[&item_deepseek, &item_kimi])
                .build()?;
            app.menu().expect("default menu").append(&tabs_menu)?;
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
