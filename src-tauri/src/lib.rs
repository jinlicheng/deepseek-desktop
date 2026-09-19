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

fn tab_url(name: &str) -> &'static str {
    match name {
        TAB_KIMI => "https://www.kimi.com",
        _ => "https://www.deepseek.com",
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
    let scale = win.scale_factor()?;
    let size = win.inner_size()?.to_logical::<f64>(scale);

    let handler_app = app.clone();
    let builder = tauri::WebviewBuilder::new(name, WebviewUrl::External(tab_url(name).parse().unwrap()))
        .auto_resize()
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

    win.add_child(
        builder,
        LogicalPosition::new(0.0, TABBAR_HEIGHT),
        LogicalSize::new(size.width, (size.height - TABBAR_HEIGHT).max(0.0)),
    )?;
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![switch_tab, reload_tab])
        .on_window_event(|window, event| {
            // auto_resize 之外的保险：窗口缩放时手动同步两个子 webview
            if let tauri::WindowEvent::Resized(_) = event {
                if let (Ok(scale), Ok(size)) = (window.scale_factor(), window.inner_size()) {
                    let size = size.to_logical::<f64>(scale);
                    for name in [TAB_DEEPSEEK, TAB_KIMI] {
                        if let Some(wv) = window.get_webview(name) {
                            let _ = wv.set_position(LogicalPosition::new(0.0, TABBAR_HEIGHT));
                            let _ = wv.set_size(LogicalSize::new(
                                size.width,
                                (size.height - TABBAR_HEIGHT).max(0.0),
                            ));
                        }
                    }
                }
            }
        })
        .setup(|app| {
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
