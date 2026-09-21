use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use tauri::menu::{MenuItemBuilder, Submenu};
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::config::{new_tab_id, AppConfig, TabConfig};
use crate::{set_panel_str, MENU_TAB_PREFIX, WIN_LABEL};

/// 同时打开的标签页上限（快捷键 Cmd/Ctrl+1~9 也按此设计）
pub const MAX_OPEN_TABS: usize = 9;

/// 当前打开的一个标签页
#[derive(Debug, Clone, Serialize)]
pub struct OpenTab {
    pub id: String,
    pub name: String,
    pub url: String,
    /// 常驻 = 来自配置；临时 = 下拉打开或未勾选保存
    pub pinned: bool,
}

/// 标签页集合 + 应用配置（被 manage 进应用状态）
pub struct Tabs {
    pub config: AppConfig,
    pub config_path: PathBuf,
    pub open: Vec<OpenTab>,
    pub active: Option<String>,
    /// 需要先销毁再创建的 webview 标签（如修改网址后）
    recreate: Vec<String>,
}

/// 标签栏右侧停靠面板的种类
#[derive(Debug, Clone, Copy)]
pub enum Panel {
    Add,
    Available,
    Settings,
}

impl Panel {
    pub fn as_str(self) -> &'static str {
        match self {
            Panel::Add => "add",
            Panel::Available => "available",
            Panel::Settings => "settings",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "add" => Some(Panel::Add),
            "available" => Some(Panel::Available),
            "settings" => Some(Panel::Settings),
            _ => None,
        }
    }
}

/// 「标签」菜单（含 Cmd/Ctrl+1~9 快捷键），启动时创建，随后按需重建
pub struct TabMenu(pub Submenu<Wry>);

pub fn label_of(tab_id: &str) -> String {
    format!("tab_{tab_id}")
}

impl Tabs {
    pub fn load(config_path: PathBuf) -> Self {
        let config = AppConfig::load(&config_path);
        let open: Vec<OpenTab> = config
            .tabs
            .iter()
            .take(config.startup_count)
            .map(|t| OpenTab {
                id: t.id.clone(),
                name: t.name.clone(),
                url: t.url.clone(),
                pinned: true,
            })
            .collect();
        let active = open.first().map(|t| t.id.clone());
        Self {
            config,
            config_path,
            open,
            active,
            recreate: Vec::new(),
        }
    }

    fn save(&self) -> Result<(), String> {
        self.config.save(&self.config_path).map_err(|e| e.to_string())
    }

    /// 配置里尚未打开的标签（「▾」列表内容）
    pub fn available(&self) -> Vec<TabConfig> {
        self.config
            .tabs
            .iter()
            .filter(|t| !self.is_open(&t.id))
            .cloned()
            .collect()
    }

    pub fn is_open(&self, id: &str) -> bool {
        self.open.iter().any(|t| t.id == id)
    }
}

/// 推送给前端的状态快照
#[derive(Debug, Clone, Serialize)]
pub struct TabsSnapshot {
    pub startup_count: usize,
    pub tabs: Vec<TabConfig>,
    pub open: Vec<OpenTab>,
    pub active: Option<String>,
    pub available: Vec<TabConfig>,
    pub panel: Option<&'static str>,
}

pub fn snapshot(app: &AppHandle) -> TabsSnapshot {
    let state = app.state::<Mutex<Tabs>>();
    let tabs = state.lock().unwrap();
    TabsSnapshot {
        startup_count: tabs.config.startup_count,
        tabs: tabs.config.tabs.clone(),
        open: tabs.open.clone(),
        active: tabs.active.clone(),
        available: tabs.available(),
        panel: crate::current_panel(app).map(|p| p.as_str()),
    }
}

pub fn emit_state(app: &AppHandle) {
    let _ = app.emit("state-changed", snapshot(app));
    rebuild_tab_menu(app);
}

/// 让「标签」菜单与当前打开的标签一致（名称 + Cmd/Ctrl+1~9 快捷键）
fn rebuild_tab_menu(app: &AppHandle) {
    let handle = app.clone();
    let app = app.clone();
    let _ = handle.run_on_main_thread(move || {
        let Some(tab_menu) = app.try_state::<TabMenu>() else {
            return;
        };
        let tab_menu = tab_menu.0.clone();
        let open = {
            let state = app.state::<Mutex<Tabs>>();
            let tabs = state.lock().unwrap();
            tabs.open.clone()
        };
        if let Ok(items) = tab_menu.items() {
            for item in items {
                let _ = tab_menu.remove(&item);
            }
        }
        for (i, t) in open.iter().enumerate() {
            if i >= MAX_OPEN_TABS {
                break;
            }
            let Ok(item) =
                MenuItemBuilder::with_id(format!("{MENU_TAB_PREFIX}{}", t.id), t.name.clone())
                    .accelerator(format!("CmdOrCtrl+{}", i + 1))
                    .build(&app)
            else {
                continue;
            };
            let _ = tab_menu.append(&item);
        }
    });
}

/// 让 webview 与 open/active 列表一致：销毁多余的、创建缺失的、调整几何与显隐。
/// Windows 上创建 webview 不能在命令或事件处理器里进行（会死锁），统一投递到主线程。
pub fn reconcile(app: &AppHandle, focus_active: bool) {
    let handle = app.clone();
    let app = app.clone();
    let _ = handle.run_on_main_thread(move || {
        let (open, active, recreate) = {
            let state = app.state::<Mutex<Tabs>>();
            let mut tabs = state.lock().unwrap();
            (
                tabs.open.clone(),
                tabs.active.clone(),
                std::mem::take(&mut tabs.recreate),
            )
        };
        let Some(win) = app.get_window(WIN_LABEL) else {
            return;
        };
        // 几何统一走 apply_bounds（它会记录 last_applied 并应用到已存在的 webview），
        // 这里拿它的返回值创建缺失的 webview
        let Some((position, size)) = crate::apply_bounds(&app) else {
            return;
        };

        // 1. 销毁：回收站标签 + 待重建列表
        let keep: Vec<String> = open.iter().map(|t| label_of(&t.id)).collect();
        for (label, wv) in app.webviews() {
            if label.starts_with("tab_") && (recreate.contains(&label) || !keep.contains(&label)) {
                let _ = wv.close();
            }
        }

        // 2. 创建缺失的，并统一几何与显隐
        for t in &open {
            let label = label_of(&t.id);
            let is_active = active.as_deref() == Some(t.id.as_str());
            let wv = match app.get_webview(&label) {
                Some(wv) => Some(wv),
                None => {
                    let Ok(url) = t.url.parse() else { continue };
                    let builder = tauri::WebviewBuilder::new(&label, WebviewUrl::External(url))
                        .on_new_window(new_window_handler(app.clone()));
                    win.add_child(builder, position, size).ok()
                }
            };
            if let Some(wv) = wv {
                let _ = wv.set_position(position);
                let _ = wv.set_size(size);
                if is_active {
                    let _ = wv.show();
                    if focus_active {
                        let _ = wv.set_focus();
                    }
                } else {
                    let _ = wv.hide();
                }
            }
        }
    });
}

/// target="_blank" / window.open 统一处理：
/// 按域名路由到对应标签的 webview，无匹配则交给系统浏览器
fn new_window_handler(
    app: AppHandle,
) -> impl Fn(Url, tauri::webview::NewWindowFeatures) -> tauri::webview::NewWindowResponse<Wry>
       + Send
       + 'static {
    move |url, _features| {
        let host = url.domain().unwrap_or_default().to_lowercase();
        let target = {
            let state = app.state::<Mutex<Tabs>>();
            let tabs = state.lock().unwrap();
            tabs.open
                .iter()
                .find(|t| {
                    Url::parse(&t.url)
                        .ok()
                        .and_then(|u| u.domain().map(str::to_lowercase))
                        .map(|d| host == d || host.ends_with(&format!(".{d}")))
                        .unwrap_or(false)
                })
                .map(|t| t.id.clone())
        };
        match target.and_then(|id| app.get_webview(&label_of(&id))) {
            Some(wv) => {
                let _ = wv.navigate(url);
            }
            None => {
                let _ = app.opener().open_url(url.as_str(), None::<&str>);
            }
        }
        tauri::webview::NewWindowResponse::Deny
    }
}

fn normalize_url(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("网址不能为空".into());
    }
    let candidate = if input.starts_with("http://") || input.starts_with("https://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };
    let parsed = Url::parse(&candidate).map_err(|_| format!("网址格式不正确：{input}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(candidate),
        _ => Err("仅支持 http/https 网址".into()),
    }
}

pub fn switch_to(app: &AppHandle, id: &str) {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        if tabs.is_open(id) {
            tabs.active = Some(id.to_string());
        }
    }
    reconcile(app, true);
    emit_state(app);
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> TabsSnapshot {
    snapshot(&app)
}

#[tauri::command]
pub fn switch_tab(app: AppHandle, id: String) {
    switch_to(&app, &id);
}

/// 刷新指定标签页（webview 页面进程被系统回收后的兜底手段）
#[tauri::command]
pub fn reload_tab(app: AppHandle, id: String) {
    if let Some(wv) = app.get_webview(&label_of(&id)) {
        let _ = wv.reload();
    }
}

#[tauri::command]
pub fn open_available(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        if !tabs.is_open(&id) {
            let cfg = tabs
                .config
                .tabs
                .iter()
                .find(|t| t.id == id)
                .cloned()
                .ok_or("配置中不存在该站点")?;
            if tabs.open.len() >= MAX_OPEN_TABS {
                return Err(format!(
                    "同时打开的标签页已达上限（{MAX_OPEN_TABS} 个），请先关闭一些"
                ));
            }
            tabs.open.push(OpenTab {
                id: cfg.id,
                name: cfg.name,
                url: cfg.url,
                pinned: false,
            });
        }
        tabs.active = Some(id);
    }
    // 打开动作完成后收起面板，让网页占满内容区
    set_panel_str(&app, None);
    reconcile(&app, true);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn close_tab(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let Some(pos) = tabs.open.iter().position(|t| t.id == id) else {
            return Ok(());
        };
        if tabs.open[pos].pinned {
            return Err("常驻标签页不能关闭，可在设置中删除".into());
        }
        tabs.open.remove(pos);
        if tabs.active.as_deref() == Some(id.as_str()) {
            tabs.active = tabs.open.first().map(|t| t.id.clone());
        }
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn add_tab(app: AppHandle, name: String, url: String, save: bool) -> Result<String, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    let url = normalize_url(&url)?;
    let new_id = new_tab_id();
    let added = {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        if let Some(existing) = tabs.open.iter().find(|t| t.url == url) {
            tabs.active = Some(existing.id.clone());
            false
        } else {
            if tabs.open.len() >= MAX_OPEN_TABS {
                return Err(format!(
                    "同时打开的标签页已达上限（{MAX_OPEN_TABS} 个），请先关闭一些"
                ));
            }
            if save {
                tabs.config.tabs.push(TabConfig {
                    id: new_id.clone(),
                    name: name.clone(),
                    url: url.clone(),
                });
                tabs.save()?;
            }
            tabs.open.push(OpenTab {
                id: new_id.clone(),
                name,
                url,
                pinned: save,
            });
            tabs.active = Some(new_id.clone());
            true
        }
    };
    if added {
        set_panel_str(&app, None);
    }
    reconcile(&app, true);
    emit_state(&app);
    Ok(new_id)
}

#[tauri::command]
pub fn update_tab(
    app: AppHandle,
    id: String,
    name: Option<String>,
    url: Option<String>,
) -> Result<(), String> {
    let url = url.map(|u| normalize_url(&u)).transpose()?;
    let recreate = {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let (new_name, new_url, url_changed) = {
            let Some(cfg) = tabs.config.tabs.iter_mut().find(|t| t.id == id) else {
                return Err("配置中不存在该站点".into());
            };
            let old_url = cfg.url.clone();
            if let Some(n) = name {
                cfg.name = n.trim().to_string();
            }
            if let Some(u) = &url {
                cfg.url = u.clone();
            }
            (
                cfg.name.clone(),
                cfg.url.clone(),
                url.is_some() && url.as_ref() != Some(&old_url),
            )
        };
        tabs.save()?;
        let mut recreate = false;
        if let Some(open) = tabs.open.iter_mut().find(|o| o.id == id) {
            open.name = new_name;
            open.url = new_url;
            recreate = url_changed;
        }
        recreate
    };
    if recreate {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        tabs.recreate.push(label_of(&id));
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn delete_tab(app: AppHandle, id: String) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        if !tabs.config.tabs.iter().any(|t| t.id == id) {
            return Err("配置中不存在该站点".into());
        }
        tabs.config.tabs.retain(|t| t.id != id);
        tabs.config.startup_count = tabs.config.startup_count.min(tabs.config.tabs.len());
        tabs.save()?;
        tabs.open.retain(|o| o.id != id);
        if tabs.active.as_deref() == Some(id.as_str()) {
            tabs.active = tabs.open.first().map(|t| t.id.clone());
        }
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn move_tab(app: AppHandle, id: String, up: bool) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let pos = tabs
            .config
            .tabs
            .iter()
            .position(|t| t.id == id)
            .ok_or("配置中不存在该站点")?;
        let new_pos = if up {
            pos.saturating_sub(1)
        } else {
            (pos + 1).min(tabs.config.tabs.len().saturating_sub(1))
        };
        if new_pos != pos {
            tabs.config.tabs.swap(pos, new_pos);
        }
        tabs.save()?;
        // 已打开的常驻标签按配置顺序重排（临时标签保持在尾部，sort_by_key 稳定）
        let order: Vec<String> = tabs.config.tabs.iter().map(|t| t.id.clone()).collect();
        tabs.open.sort_by_key(|o| {
            if !o.pinned {
                usize::MAX
            } else {
                order.iter().position(|x| x == &o.id).unwrap_or(usize::MAX)
            }
        });
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn set_startup_count(app: AppHandle, n: usize) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let n = n.min(tabs.config.tabs.len()).min(MAX_OPEN_TABS);
        tabs.config.startup_count = n;
        tabs.save()?;
        // 让已打开的常驻标签与「配置前 n 个」一致
        let wanted: Vec<String> = tabs
            .config
            .tabs
            .iter()
            .take(n)
            .map(|t| t.id.clone())
            .collect();
        tabs.open.retain(|o| !o.pinned || wanted.contains(&o.id));
        for id in wanted {
            if !tabs.is_open(&id) {
                let cfg = tabs
                    .config
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
                    .cloned()
                    .unwrap();
                tabs.open.push(OpenTab {
                    id: cfg.id,
                    name: cfg.name,
                    url: cfg.url,
                    pinned: true,
                });
            }
        }
        if tabs.active.is_none() {
            tabs.active = tabs.open.first().map(|t| t.id.clone());
        }
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

#[tauri::command]
pub fn set_panel(app: AppHandle, panel: Option<String>) {
    set_panel_str(&app, panel.as_deref().and_then(Panel::parse));
    crate::apply_bounds(&app);
    emit_state(&app);
}
