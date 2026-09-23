//! 统一下载：所有站点的下载都落到配置的目录（缺省为系统下载目录），
//! 可选按站点建立子目录；下载完成时向前端推送事件用于提示。
//!
//! 之所以必须有这个处理器：wry 只有在设置了下载处理器时才会把导航策略判为
//! `Download`，否则（macOS 上）直接 `Cancel` —— 站点里的下载链接会毫无反应。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use tauri::webview::DownloadEvent;
use tauri::{AppHandle, Emitter, Manager, Url, Webview, Wry};

use crate::tabs::Tabs;

/// 推送给前端的下载完成信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadInfo {
    pub name: String,
    pub success: bool,
}

/// 已受理下载的目标路径，按 URL 记录：macOS 的 Finished 事件拿不到保存路径
fn pending() -> &'static Mutex<HashMap<String, PathBuf>> {
    static PENDING: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 最近一次成功下载的文件，供「立即查看」定位
pub fn last_download() -> Option<PathBuf> {
    last_slot().lock().unwrap().clone()
}

fn last_slot() -> &'static Mutex<Option<PathBuf>> {
    static LAST: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
    LAST.get_or_init(|| Mutex::new(None))
}

pub fn handler(
    app: AppHandle,
) -> impl for<'a> Fn(Webview<Wry>, DownloadEvent<'a>) -> bool + Send + Sync + 'static {
    move |webview, event| match event {
        DownloadEvent::Requested { url, destination } => {
            match plan(&app, webview.label(), &url, destination) {
                Ok(target) => {
                    crate::debug_log(&format!(
                        "[download] label={} url={} -> {}",
                        webview.label(),
                        url,
                        target.display()
                    ));
                    pending()
                        .lock()
                        .unwrap()
                        .insert(url.to_string(), target.clone());
                    *destination = target;
                    true
                }
                // 落点算不出来（目录不可建等）就拒绝，避免文件落到意外位置
                Err(e) => {
                    crate::debug_log(&format!("[download] 落点计算失败: {e}"));
                    false
                }
            }
        }
        DownloadEvent::Finished { url, success, .. } => {
            let path = pending().lock().unwrap().remove(url.as_str());
            let name = path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "文件".to_string());
            if success {
                if let Some(p) = &path {
                    *last_slot().lock().unwrap() = Some(p.clone());
                }
            }
            crate::debug_log(&format!("[download-finished] {url} success={success} path={path:?}"));
            let _ = app.emit(
                "download-finished",
                DownloadInfo { name, success },
            );
            true
        }
        // DownloadEvent 是 non_exhaustive：日后新增的变体默认放行
        _ => {
            crate::debug_log("[download-other] 未知事件变体");
            true
        }
    }
}

/// 计算落点：下载目录（配置或系统默认）[/站点名]/文件名，重名按 " (n)" 后缀
fn plan(app: &AppHandle, label: &str, url: &Url, suggested: &Path) -> Result<PathBuf, String> {
    let name = suggested
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    plan_named(app, label, url, &name)
}

/// 落点目录：不存在的目录会被创建
fn dir_for(app: &AppHandle, label: &str, url: &Url) -> Result<PathBuf, String> {
    let (configured, per_site) = {
        let state = app.state::<Mutex<Tabs>>();
        let tabs = state.lock().unwrap();
        (
            tabs.config.download_dir.clone(),
            tabs.config.download_per_site,
        )
    };
    let mut base = match configured {
        Some(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => app.path().download_dir().map_err(|e| e.to_string())?,
    };
    if per_site {
        base.push(site_folder(app, label, url));
    }
    std::fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    Ok(base)
}

fn plan_named(app: &AppHandle, label: &str, url: &Url, name: &str) -> Result<PathBuf, String> {
    Ok(unique_path(&dir_for(app, label, url)?, &sanitize(name, "download")))
}

/// 站点子目录名：优先用标签名（如 DeepSeek），回退到域名
fn site_folder(app: &AppHandle, label: &str, url: &Url) -> String {
    let tab_name = tab_id_of(label).and_then(|tab_id| {
        let state = app.state::<Mutex<Tabs>>();
        let tabs = state.lock().unwrap();
        tabs.open
            .iter()
            .find(|t| t.id == tab_id)
            .map(|t| t.name.clone())
    });
    match tab_name.filter(|n| !n.trim().is_empty()) {
        Some(name) => sanitize(&name, "站点"),
        None => sanitize(url.host_str().unwrap_or("站点"), "站点"),
    }
}

/// 从 webview 标签取标签页 id：标签页是 `tab_<id>`，下载兜底 webview 是 `dl_<id>__<n>`
fn tab_id_of(label: &str) -> Option<&str> {
    let rest = label
        .strip_prefix("tab_")
        .or_else(|| label.strip_prefix("dl_"))?;
    Some(rest.split("__").next().unwrap_or(rest))
}

/// 重名时按 wry 的约定加 " (n)"
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = split_name(name);
    for i in 1..10_000 {
        let candidate = dir.join(format!("{stem} ({i}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    candidate
}

fn split_name(name: &str) -> (String, String) {
    match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.to_string(), String::new()),
    }
}

/// 去掉路径分隔符、控制字符与首尾点，避免逃出目标目录；过长时截断
fn sanitize(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

/// 注入到每个标签页（含子框架）文档开头的下载接管脚本。
///
/// 站点（如 Kimi）的下载按钮是「造一个 `<a target="_blank">` 再 `a.click()`」。这种程序化
/// 触发的新窗口会被 WebKit 静默拦掉——我们既拿不到新窗口回调、也拿不到导航事件，下载就
/// 凭空消失（实测：点 Kimi 的下载按钮，Rust 侧一条日志都没有）。所以只能在页面里接管：
///   - 同源文件地址：自己 fetch 回来，用带 download 属性的 blob 链接触发下载——同源的
///     `download` 属性一定生效，会走到统一下载处理器；
///   - 跨域文件地址：把地址通过哨兵地址发回 Rust，交给那边的隐藏 webview 兜底（跨域 fetch
///     会被 CORS 挡掉，只能由 Rust 侧发起）。
/// 只接管「像文件」的地址，其它链接一律原样放行。
///
/// 结构上刻意保持**只有一个 IIFE**：之前把「接管」和「诊断」拼成两个 IIFE 时，
/// 实测只有第一个会执行（第二个整段不跑），合成一个最稳。
///
/// **不要在页面侧做诊断上报**：早先的探测用 `location.href = 'https://<哨兵>/...'` 回传，
/// 那是一次导航尝试，会在站点造 blob／发起下载的瞬间把流程打断（实测产生"单击无效、双击才行"
/// 的假 bug）。需要页面侧信息时，请走 Rust 侧收报文（本地 HTTP 监听 + fetch），别碰导航。
pub const INJECTED_JS: &str = r#"(() => {
  const CHANNEL = 'jai-dl.invalid';

  // ---- 接管逻辑 ----
  const notify = (href) => { setTimeout(() => { try { location.href = 'https://' + CHANNEL + '/dl-' + encodeURIComponent(href); } catch (e) {} }, 0); };
  const lastSegment = (url) => {
    const raw = url.pathname.split('/').filter(Boolean).pop() || '';
    let decoded = raw;
    try { decoded = decodeURIComponent(raw); } catch (e) {}
    const parts = decoded.split('/').filter(Boolean);
    return parts[parts.length - 1] || '';
  };
  // 文件扩展名白名单（由 Rust 侧 FILE_EXTENSIONS 注入，保证只有一份）：不接管 .html 这类页面地址
  const EXTS = __JAI_EXTS__;
  const looksLikeFile = (url) => {
    const last = lastSegment(url).toLowerCase();
    const dot = last.lastIndexOf('.');
    if (dot >= 0 && EXTS.indexOf(last.slice(dot + 1)) >= 0) return true;
    const q = url.search.toLowerCase();
    return q.includes('filename=') || q.includes('disposition') || q.includes('attachment') || q.includes('download=');
  };
  const MIME_EXT = {
    'text/markdown': '.md', 'text/plain': '.txt', 'text/csv': '.csv', 'application/json': '.json',
    'application/pdf': '.pdf', 'application/zip': '.zip', 'image/png': '.png', 'image/jpeg': '.jpg',
    'image/gif': '.gif', 'image/webp': '.webp', 'audio/mpeg': '.mp3', 'video/mp4': '.mp4',
  };
  const fileNameFrom = (cd, url, mime) => {
    const m = cd.match(/filename\*=UTF-8''([^;]+)/i) || cd.match(/filename="?([^";]+)"?/i);
    let name = '';
    if (m) { try { name = decodeURIComponent(m[1]); } catch (e) { name = m[1]; } }
    if (name) return name;
    const q = url.searchParams.get('filename');
    if (q) return q;
    const last = lastSegment(url);
    if (last.includes('.')) return last;
    // blob: 地址没有文件名，也没有响应头，只能靠类型兜一个后缀
    return 'download' + (MIME_EXT[String(mime).toLowerCase()] || '');
  };
  const takeOver = (url, suggested) => {
    const isBlob = url.protocol === 'blob:' || url.protocol === 'data:';
    if (!isBlob && url.origin !== location.origin) { notify(url.href); return; }
    fetch(url.href, { credentials: 'include' })
      .then((res) => res.blob().then((blob) => ({ blob: blob, cd: res.headers.get('content-disposition') || '' })))
      .then((r) => {
        const a = document.createElement('a');
        a.href = URL.createObjectURL(r.blob);
        a.download = suggested || fileNameFrom(r.cd, url, r.blob.type);
        a.style.display = 'none';
        document.body.appendChild(a);
        a.click();
        a.remove();
        setTimeout(() => { try { URL.revokeObjectURL(a.href); } catch (e) {} }, 30000);
      })
      .catch((e) => {
        // 页面取不到（多为跨域跳转被 CORS 拦）→ 交给 Rust 原生下载
        notify(url.href);
      });
  };
  const intercept = (raw, suggested) => {
    let url = null;
    try { url = new URL(String(raw), location.href); } catch (e) {
      return false;
    }
    if (url.protocol === 'blob:' || url.protocol === 'data:') {
      // 带 download 属性的 blob 下载 WebKit 自己能行；没有属性时程序化触发会被静默吞掉
      if (suggested) return false;
      takeOver(url, '');
      return true;
    }
    if (url.protocol !== 'http:' && url.protocol !== 'https:') return false;
    if (!looksLikeFile(url)) return false;
    takeOver(url, suggested);
    return true;
  };
  const patch = (fn) => { try { fn(); } catch (e) {} };
  if (!window.__jaiDownloadHelper) {
    window.__jaiDownloadHelper = 1;
    patch(() => {
      const click = HTMLAnchorElement.prototype.click;
      HTMLAnchorElement.prototype.click = function () {
        if (this.href && intercept(this.href, this.download)) return;
        return click.apply(this);
      };
    });
    patch(() => {
      const open = window.open;
      window.open = function (u, ...rest) {
        if (typeof u === 'string' && intercept(u, '')) return null;
        return open.apply(this, [u, ...rest]);
      };
    });
    patch(() => {
      const desc = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, 'src');
      if (desc && desc.set) {
        Object.defineProperty(HTMLIFrameElement.prototype, 'src', {
          configurable: true,
          enumerable: desc.enumerable,
          get() { return desc.get.call(this); },
          set(v) { if (!intercept(v, '')) desc.set.call(this, v); },
        });
      }
    });
  } else {
  }
})()"#;

/// 注入脚本（诊断开关编译进脚本常量，避免依赖环境变量在页面侧再判断）
pub fn injected_script() -> String {
    INJECTED_JS.replace("__JAI_EXTS__", &file_extensions_js())
}

/// 会被当作「文件」的扩展名。只此一份：Rust 的导航拦截（[`crate::tabs`]）与注入脚本共用。
pub(crate) const FILE_EXTENSIONS: &[&str] = &[
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "csv", "txt", "md", "rtf", "odt", "ods",
    "odp", "zip", "rar", "7z", "tar", "gz", "bz2", "png", "jpg", "jpeg", "gif", "webp", "bmp",
    "svg", "ico", "mp3", "wav", "m4a", "aac", "flac", "mp4", "mov", "m4v", "webm", "avi", "mkv",
    "apk", "dmg", "exe", "msi", "deb", "rpm", "epub", "mobi", "woff", "woff2", "ttf",
];

fn file_extensions_js() -> String {
    let list: Vec<String> = FILE_EXTENSIONS.iter().map(|ext| format!("'{ext}'")).collect();
    format!("[{}]", list.join(","))
}

/// 走系统 curl 的原生下载。
///
/// 为什么必须有这条路：站点（如 Kimi）的下载地址是同源 URL，会 307 跳到跨域对象存储
/// （火山 TOS）。页面里的 `fetch` 遇到跨域跳转会被 CORS 直接掐断（实测 `TypeError: Load failed`），
/// 而响应类型是 `image/png` 这种"能显示的类型"时，WebView 又只会显示不会下载。
/// 原生 HTTP 客户端没有这些限制：能跟随跳转、能读 `Content-Disposition` 拿真文件名。
/// 不依赖 Cookie（这类地址都带签名），所以不需要复用 WebView 的会话。
pub fn fetch_native(app: &AppHandle, label: &str, url: &Url) -> Result<PathBuf, String> {
    let dir = dir_for(app, label, url)?;
    let stamp = format!("{}-{}", std::process::id(), url.path().len());
    let body = dir.join(format!(".jai-{stamp}.part"));
    let headers = dir.join(format!(".jai-{stamp}.head"));
    let _ = std::fs::remove_file(&body);
    let _ = std::fs::remove_file(&headers);

    let status = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "900", "-A", USER_AGENT, "-e"])
        .arg(referer_of(url))
        .arg("-D")
        .arg(&headers)
        .arg("-o")
        .arg(&body)
        .arg(url.as_str())
        .status()
        .map_err(|e| format!("无法执行 curl（{e}）"))?;
    let _cleanup = Cleanup(vec![body.clone(), headers.clone()]);
    if !status.success() {
        return Err(format!("curl 退出码 {status}"));
    }
    let head = std::fs::read_to_string(&headers).unwrap_or_default();
    let last = last_header_block(&head);
    let content_type = header_value(last, "content-type").unwrap_or_default();
    let disposition = header_value(last, "content-disposition").unwrap_or_default();
    // 没拿到附件头又是一份 HTML，多半是个错误页/登录页，不算下载成功
    if !disposition.to_ascii_lowercase().contains("attachment")
        && content_type.to_ascii_lowercase().contains("html")
    {
        return Err(format!("响应不是文件（{content_type}）"));
    }
    if std::fs::metadata(&body).map(|m| m.len()).unwrap_or(0) == 0 {
        return Err("下载内容为空".into());
    }
    let name = disposition_name(&disposition)
        .or_else(|| {
            url.query_pairs()
                .find(|(k, _)| k.eq_ignore_ascii_case("filename"))
                .map(|(_, v)| v.to_string())
        })
        .unwrap_or_else(|| {
            url.path_segments()
                .and_then(|s| s.last())
                .filter(|s| s.contains('.'))
                .unwrap_or("download")
                .to_string()
        });
    let target = plan_named(app, label, url, &name)?;
    std::fs::rename(&body, &target).map_err(|e| e.to_string())?;

    let now = target.file_name().map(|n| n.to_string_lossy().to_string());
    *last_slot().lock().unwrap() = Some(target.clone());
    let _ = app.emit(
        "download-finished",
        DownloadInfo {
            name: now.unwrap_or_else(|| "文件".into()),
            success: true,
        },
    );
    Ok(target)
}

struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";

fn referer_of(url: &Url) -> String {
    match url.port() {
        Some(port) => format!("{}://{}:{port}/", url.scheme(), url.host_str().unwrap_or("")),
        None => format!("{}://{}/", url.scheme(), url.host_str().unwrap_or("")),
    }
}

/// curl -D 会写下每一跳的响应头：取最后一块（也就是最终响应）
fn last_header_block(raw: &str) -> &str {
    raw.split("\r\n\r\n")
        .filter(|b| b.trim_start().starts_with("HTTP/"))
        .last()
        .unwrap_or(raw)
}

fn header_value<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    block.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim())
    })
}

/// 从 Content-Disposition 里取文件名（优先 filename*，其次 filename）
fn disposition_name(disposition: &str) -> Option<String> {
    let lower = disposition.to_ascii_lowercase();
    for (key, value) in disposition.split(';').filter_map(|part| part.split_once('=')) {
        let key = key.trim().to_ascii_lowercase();
        let value = value.trim().trim_matches('"');
        if key == "filename*" {
            // filename*=UTF-8''name.png
            let raw = value.splitn(3, '\'').nth(2).unwrap_or(value);
            if let Ok(decoded) = decode_percent(raw) {
                return Some(decoded);
            }
        } else if key == "filename" && !value.is_empty() {
            return Some(value.to_string());
        }
    }
    let _ = lower;
    None
}

/// 极简百分号解码（只处理 %XX，不引入额外依赖）；页面回传的地址也用它
pub(crate) fn decode_percent(raw: &str) -> Result<String, ()> {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            match u8::from_str_radix(&raw[i + 1..i + 3], 16) {
                Ok(byte) => {
                    out.push(byte);
                    i += 3;
                    continue;
                }
                Err(_) => return Err(()),
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).map_err(|_| ())
}
