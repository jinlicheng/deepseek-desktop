//! 系统托盘：点窗口关闭按钮只隐藏窗口，只有托盘菜单的「退出」才真正结束进程。
//!
//! Linux 上托盘依赖桌面环境（GNOME 需要 AppIndicator 扩展），没有托盘时窗口一旦隐藏
//! 就无法唤回，因此 Linux 默认不启用托盘、关闭窗口即退出。要在 Linux 启用，让
//! [`enabled`] 返回 true 即可（然后把托盘图标交给目标桌面环境验证）。

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

use crate::WIN_LABEL;

const MENU_TRAY_SHOW: &str = "tray-show";
const MENU_TRAY_QUIT: &str = "tray-quit";

/// 是否处于「真正退出」流程：托盘菜单的「退出」先置位，
/// 之后关闭窗口与退出请求都不再拦截
static QUITTING: AtomicBool = AtomicBool::new(false);

/// 是否启用托盘（Linux 默认不启用，见模块说明）
pub fn enabled() -> bool {
    !cfg!(target_os = "linux")
}

pub fn is_quitting() -> bool {
    QUITTING.load(Ordering::Relaxed)
}

/// 显示并聚焦主窗口（托盘左键、托盘菜单、macOS 点 Dock 图标都走这里）
pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_window(WIN_LABEL) {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 隐藏主窗口（关闭按钮与用户主动退出都走这里）
pub fn hide_main_window(app: &AppHandle) {
    if let Some(win) = app.get_window(WIN_LABEL) {
        let _ = win.hide();
    }
}

/// 真正退出应用
pub fn quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::Relaxed);
    app.exit(0);
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItemBuilder::with_id(MENU_TRAY_SHOW, "显示主窗口").build(app)?;
    let quit_item = MenuItemBuilder::with_id(MENU_TRAY_QUIT, "退出").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&show, &quit_item]).build()?;

    let mut builder = TrayIconBuilder::with_id("jai-tray")
        .menu(&menu)
        // 左键唤回窗口、右键弹菜单（三平台统一语义）
        .show_menu_on_left_click(false)
        .tooltip("JAI")
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_TRAY_SHOW => show_main_window(app),
            MENU_TRAY_QUIT => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    // 复用应用图标；macOS 按模板图渲染（取 alpha 轮廓），因此暂时无需单独的单色图标
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone()).icon_as_template(true);
    }
    builder.build(app)?;
    Ok(())
}
