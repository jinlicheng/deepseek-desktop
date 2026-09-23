use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::Serialize;
use tauri::menu::{MenuItemBuilder, Submenu};
use tauri::webview::{DownloadEvent, PageLoadEvent};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, Wry};
use tauri_plugin_opener::OpenerExt;

use crate::config::{new_tab_id, AppConfig, TabConfig};
use crate::{set_panel_str, MENU_TAB_PREFIX, WIN_LABEL};

/// 同时打开的标签页上限（快捷键 Cmd/Ctrl+1~9 也按此设计）
pub const MAX_OPEN_TABS: usize = 9;

/// 当前打开的一个标签页。数组顺序就是标签栏顺序，也是唯一的排序真相
#[derive(Debug, Clone, Serialize)]
pub struct OpenTab {
    pub id: String,
    pub name: String,
    pub url: String,
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
    pub download_dir: Option<String>,
    pub download_per_site: bool,
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
        download_dir: tabs.config.download_dir.clone(),
        download_per_site: tabs.config.download_per_site,
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
                        .initialization_script_for_all_frames(crate::download::injected_script())
                        .on_new_window(new_window_handler(app.clone()))
                        .on_navigation(tab_navigation_handler(app.clone(), label.clone()))
                        .on_download(crate::download::handler(app.clone()));
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

/// 标签页内的导航拦截：
/// 站点触发下载还有另外两种方式——隐藏 iframe 的 `src`、或直接跳转过去（不带 target）。
/// 这两种都会走「导航」，而 wry 只在 MIME 无法显示时才判成下载，所以 `image/png`
/// 这类能显示的附件会被直接显示出来、什么都不会保存。这里把「跨域 + 像文件」的导航
/// 拦下来，交给同一套下载兜底（它会按响应头决定是文件还是网页）。
///
/// 另外它还兼作页面脚本的回传通道：注入脚本遇到**跨域**文件地址时没法自己取（CORS），
/// 就用 [`CHANNEL_HOST`] 下的哨兵地址把地址发回来，这里解析后走同一条兜底路径。
fn tab_navigation_handler(
    app: AppHandle,
    label: String,
) -> impl Fn(&Url) -> bool + Send + Sync + 'static {
    move |url| {
        if url.host_str() == Some(CHANNEL_HOST) {
            let path = url.path().trim_start_matches('/');
            if let Some(raw) = path.strip_prefix("dl-") {
                // 注入脚本用 encodeURIComponent 编码，这里要显式解码
                // （path_segments() 不做百分号解码）
                let decoded = crate::download::decode_percent(raw).unwrap_or_default();
                if let Ok(target) = Url::parse(&decoded) {
                    crate::debug_log(&format!("[page-download] {target}"));
                    download_external(&app, target);
                }
            }
            return false;
        }
        if !matches!(url.scheme(), "http" | "https") {
            return true;
        }
        crate::debug_log(&format!("[nav] {label} {url}"));
        let here = app
            .get_webview(&label)
            .and_then(|wv| wv.url().ok())
            .and_then(|u| u.host_str().map(str::to_string));
        if here.as_deref() != url.host_str() && looks_like_file(url) {
            crate::debug_log(&format!("[nav-download] {url}"));
            download_external(&app, url.clone());
            return false;
        }
        true
    }
}

/// 像文件的地址：路径末段带常见扩展名，或查询串里有下载相关关键字
fn looks_like_file(url: &Url) -> bool {
    let extensions = crate::download::FILE_EXTENSIONS;
    let path = url.path().to_ascii_lowercase();
    let last = path.rsplit('/').next().unwrap_or_default();
    if let Some((_, ext)) = last.rsplit_once('.') {
        if extensions.contains(&ext) {
            return true;
        }
    }
    url.query_pairs().any(|(k, v)| {
        let k = k.to_ascii_lowercase();
        let v = v.to_ascii_lowercase();
        k.contains("disposition")
            || k.contains("attachment")
            || k.contains("download")
            || k.contains("filename")
            || v.contains("attachment")
    })
}

/// target="_blank" / window.open 统一处理：
/// 按域名路由到对应标签的 webview；跨域地址先按「下载」试一次，不是下载才交给系统浏览器
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
            // 站内地址：留在标签页里打开
            Some(wv) => {
                crate::debug_log(&format!("[new-window] {url} -> 站内标签 {}", wv.label()));
                let _ = wv.navigate(url);
            }
            None => {
                crate::debug_log(&format!("[new-window] {url} -> 下载"));
                download_external(&app, url);
            }
        }
        tauri::webview::NewWindowResponse::Deny
    }
}

/// 跨域/带签名的文件地址：先用 Rust 原生 HTTP 抓（页面 fetch 会被 CORS 拦、能显示的类型
/// WebView 又不会下载），失败再退回隐藏 webview 兜底，最后才交给系统浏览器。
fn download_external(app: &AppHandle, url: Url) {
    let site = active_tab_id(app).unwrap_or_default();
    let label = format!("dl_{site}__{}", next_helper_id());
    let app_for_thread = app.clone();
    let url_for_thread = url.clone();
    std::thread::spawn(move || {
        match crate::download::fetch_native(&app_for_thread, &label, &url_for_thread) {
            Ok(path) => crate::debug_log(&format!("[native-download] ok {}", path.display())),
            Err(e) => {
                crate::debug_log(&format!("[native-download] 失败: {e}，退回 webview 兜底"));
                let fallback_app = app_for_thread.clone();
                let _ = app_for_thread.run_on_main_thread(move || {
                    open_external(&fallback_app, url_for_thread);
                });
            }
        }
    });
}

/// 「还没判成下载」的兜底 webview：只有它们才允许被超时清理，
/// 否则可能打断正在进行中的下载
fn probing() -> &'static Mutex<HashSet<String>> {
    static PROBING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PROBING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 页面脚本 → Rust 的回传通道主机（`.invalid` 是保留域名，绝不会真的解析请求）
const CHANNEL_HOST: &str = "jai-dl.invalid";

/// 跨域新窗口请求的下载兜底。
///
/// 站点（如 Kimi 的文件卡片）的下载按钮是 `<a href="<CDN 地址>" target="_blank">`，
/// WebKit 把它判成「新窗口」而不是下载，于是我们拿不到它的响应头——按老办法交给系统
/// 浏览器，文件就落到浏览器自己的下载目录（甚至是只被浏览器打开、根本没保存）。
///
/// 这里改成一个隐藏的小 webview 来处理：
/// 1. 先用它的**站点根地址**加载一次，拿到一个可执行脚本的文档（跨域时同源脚本才读得到响应头）；
/// 2. 注入脚本对文件地址做同源 fetch：带 `Content-Disposition: attachment` 或不是 HTML 的，
///    取回内容后用 `<a download>` 触发下载（wry 只看 MIME 决定是否下载，图片这类能显示的类型
///    会直接被显示出来而不会下载，所以必须绕这一圈）；
/// 3. 不带附件的 HTML 说明是普通网页（外链）→ 用哨兵地址通知 Rust 交回系统浏览器。
fn open_external(app: &AppHandle, url: Url) {
    if !matches!(url.scheme(), "http" | "https") {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
        return;
    }
    let Some(origin) = origin_of(&url) else {
        let _ = app.opener().open_url(url.as_str(), None::<&str>);
        return;
    };
    let helper = Helper {
        label: format!(
            "dl_{}__{}",
            active_tab_id(app).unwrap_or_default(),
            next_helper_id()
        ),
        url,
    };
    probing().lock().unwrap().insert(helper.label.clone());

    let window_app = app.clone();
    let helper_for_build = helper.clone();
    let script = probe_js(helper.url.as_str());
    let _ = app.run_on_main_thread(move || {
        let Some(win) = window_app.get_window(WIN_LABEL) else {
            return;
        };
        let download_app = window_app.clone();
        let download_helper = helper_for_build.clone();
        let nav_app = window_app.clone();
        let nav_helper = helper_for_build.clone();
        let builder = tauri::WebviewBuilder::new(
            &helper_for_build.label,
            WebviewUrl::External(origin),
        )
        .on_download(move |wv, event| {
            if matches!(event, DownloadEvent::Requested { .. }) {
                crate::debug_log(&format!("[helper] {} 判定为下载", download_helper.url));
                probing().lock().unwrap().remove(&download_helper.label);
            }
            let finished = matches!(event, DownloadEvent::Finished { .. });
            let keep = (crate::download::handler(download_app.clone()))(wv, event);
            if finished {
                download_helper.close(&download_app);
                // 兜底：下载结束事件没到也不能把 webview 留在后台
                download_helper.close_later(&download_app);
            }
            keep
        })
        .on_page_load(move |wv, payload| {
            if payload.event() == PageLoadEvent::Finished {
                let _ = wv.eval(&script);
            }
        })
        .on_navigation(move |url| {
            if url.host_str() == Some(CHANNEL_HOST) {
                let why = url.path().trim_start_matches('/').to_string();
                crate::debug_log(&format!("[probe] {} -> {why}", nav_helper.url));
                if why == "enter" {
                    // 脚本跑起来了：不再需要超时兜底（大文件要慢慢取）
                    probing().lock().unwrap().remove(&nav_helper.label);
                } else {
                    let reason = if why == "external" { "是网页（外链）" } else { "取内容失败" };
                    nav_helper.to_browser(&nav_app, reason);
                }
                return false;
            }
            true
        });
        match win.add_child(
            builder,
            LogicalPosition::new(0.0, 0.0),
            LogicalSize::new(1.0, 1.0),
        ) {
            Ok(wv) => {
                let _ = wv.hide();
            }
            Err(_) => helper_for_build.to_browser(&window_app, "兜底 webview 创建失败"),
        }
    });

    // 超时兜底：脚本没能跑起来（文档不可执行、根地址也加载不出来等）就交回系统浏览器
    let timeout_app = app.clone();
    let timeout_helper = helper;
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(12));
        if probing().lock().unwrap().contains(&timeout_helper.label) {
            timeout_helper.to_browser(&timeout_app, "超时（脚本未运行）");
        }
    });
}

/// 站点根地址（scheme://host[:port]/）：同源，用来承载探测脚本
fn origin_of(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{host}{port}/", url.scheme()).parse().ok()
}

/// 探测脚本：同源取回文件，按响应头决定「下载」还是「交回浏览器」。
/// 结果用 `CHANNEL_HOST` 下的哨兵地址回传给 Rust（脚本里的导航会被取消，只是当信号用）。
fn probe_js(file_url: &str) -> String {
    let quoted = serde_json::to_string(file_url).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        r#"(async () => {{
  const mark = (m) => {{ try {{ location.href = 'https://{CHANNEL_HOST}/' + m; }} catch (e) {{}} }};
  mark('enter');
  try {{
    const res = await fetch({quoted}, {{ credentials: 'same-origin' }});
    const cd = res.headers.get('content-disposition') || '';
    const ct = (res.headers.get('content-type') || '').toLowerCase();
    if (!/attachment/i.test(cd) && ct.includes('html')) return mark('external');
    const blob = await res.blob();
    const m = cd.match(/filename\*=UTF-8''([^;]+)/i) || cd.match(/filename="?([^";]+)"?/i);
    let name = m ? decodeURIComponent(m[1]) : '';
    if (!name) {{
      const last = decodeURIComponent(new URL({quoted}).pathname.split('/').filter(Boolean).pop() || '');
      name = last.includes('.') ? last : 'download';
    }}
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = name;
    document.body.appendChild(a);
    a.click();
    a.remove();
  }} catch (e) {{
    mark('error');
  }}
}})()"#
    )
}

/// 下载兜底的隐藏 webview
#[derive(Clone)]
struct Helper {
    label: String,
    url: Url,
}

impl Helper {
    /// 收尾：关闭兜底 webview（关闭要投递到主线程，调用点可能正在 webview 回调里）
    fn close(&self, app: &AppHandle) {
        probing().lock().unwrap().remove(&self.label);
        let label = self.label.clone();
        let app = app.clone();
        let _ = app.clone().run_on_main_thread(move || {
            if let Some(wv) = app.get_webview(&label) {
                let _ = wv.close();
            }
        });
    }

    /// 兜底清理：脚本卡住（大文件、请求挂起）时也要把隐藏 webview 收掉
    fn close_later(&self, app: &AppHandle) {
        let this = self.clone();
        let app = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(600));
            this.close(&app);
        });
    }

    /// 判定为外链（或探测失败）：关掉兜底 webview，按原行为交给系统浏览器
    fn to_browser(&self, app: &AppHandle, why: &str) {
        crate::debug_log(&format!("[helper] {} {why}", self.url));
        self.close(app);
        let _ = app.opener().open_url(self.url.as_str(), None::<&str>);
    }
}

fn next_helper_id() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

fn active_tab_id(app: &AppHandle) -> Option<String> {
    let state = app.state::<Mutex<Tabs>>();
    let tabs = state.lock().unwrap();
    tabs.active.clone()
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
        if tabs.open.len() <= 1 {
            return Err("至少保留一个标签页".into());
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
    }
    reconcile(&app, false);
    emit_state(&app);
    Ok(())
}

/// 设置「启动时自动打开前 N 个」。只补不删：确保配置前 N 个都在标签栏里（缺的追加到末尾），
/// 不主动关掉其它已打开的标签——调个数字就把在用的标签关掉太意外。
#[tauri::command]
pub fn set_startup_count(app: AppHandle, n: usize) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let n = n.min(tabs.config.tabs.len()).min(MAX_OPEN_TABS);
        tabs.config.startup_count = n;
        tabs.save()?;
        let wanted: Vec<TabConfig> = tabs.config.tabs.iter().take(n).cloned().collect();
        for cfg in wanted {
            if !tabs.is_open(&cfg.id) {
                if tabs.open.len() >= MAX_OPEN_TABS {
                    break;
                }
                tabs.open.push(OpenTab {
                    id: cfg.id,
                    name: cfg.name,
                    url: cfg.url,
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

/// 拖拽排序后提交的新顺序。`open` 的顺序即标签栏顺序；同时把「配置里已打开的站点」
/// 按屏幕顺序写回配置（未打开的站点保持原有相对顺序），这样"启动打开前 N 个"与所见一致。
#[tauri::command]
pub fn set_open_order(app: AppHandle, ids: Vec<String>) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        let mut ordered: Vec<OpenTab> = Vec::with_capacity(tabs.open.len());
        for id in &ids {
            if let Some(tab) = tabs.open.iter().find(|t| &t.id == id) {
                if !ordered.iter().any(|t| &t.id == id) {
                    ordered.push(tab.clone());
                }
            }
        }
        for tab in &tabs.open {
            if !ordered.iter().any(|t| &t.id == &tab.id) {
                ordered.push(tab.clone());
            }
        }
        if ordered.len() != tabs.open.len() {
            return Err("标签顺序与当前标签不一致".into());
        }
        let mut reordered: Vec<TabConfig> = Vec::with_capacity(tabs.config.tabs.len());
        for tab in &ordered {
            if let Some(cfg) = tabs.config.tabs.iter().find(|c| c.id == tab.id) {
                reordered.push(cfg.clone());
            }
        }
        for cfg in &tabs.config.tabs {
            if !reordered.iter().any(|c| c.id == cfg.id) {
                reordered.push(cfg.clone());
            }
        }
        let changed = reordered.iter().map(|c| c.id.clone()).collect::<Vec<_>>()
            != tabs
                .config
                .tabs
                .iter()
                .map(|c| c.id.clone())
                .collect::<Vec<_>>();
        tabs.open = ordered;
        if changed {
            tabs.config.tabs = reordered;
            tabs.save()?;
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

/// 设置统一下载目录；传 None（或空串）表示回到系统默认下载目录
#[tauri::command]
pub fn set_download_dir(app: AppHandle, dir: Option<String>) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        tabs.config.download_dir = dir.filter(|d| !d.trim().is_empty());
        tabs.save()?;
    }
    emit_state(&app);
    Ok(())
}

/// 设置下载时是否按站点建立子目录
#[tauri::command]
pub fn set_download_per_site(app: AppHandle, enabled: bool) -> Result<(), String> {
    {
        let state = app.state::<Mutex<Tabs>>();
        let mut tabs = state.lock().unwrap();
        tabs.config.download_per_site = enabled;
        tabs.save()?;
    }
    emit_state(&app);
    Ok(())
}
