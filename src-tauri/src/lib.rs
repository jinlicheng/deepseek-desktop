use tauri::Manager;
use tauri_plugin_opener::OpenerExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::External("https://www.deepseek.com".parse().unwrap()),
            )
            .title("DeepSeek")
            .inner_size(1200.0, 800.0)
            .min_inner_size(800.0, 600.0)
            // 页面里的 target="_blank" / window.open 会走到这个拦截器。
            // 默认 WKWebView 会静默丢弃新窗口请求（表现为点击无反应），
            // 这里改为：DeepSeek 站内链接在当前窗口打开，站外链接交给系统浏览器。
            .on_new_window(move |url, _features| {
                let host = url.domain().unwrap_or_default().to_lowercase();
                if host.ends_with("deepseek.com") {
                    if let Some(win) = handle.get_webview_window("main") {
                        let _ = win.navigate(url);
                    }
                } else {
                    let _ = handle.opener().open_url(url.as_str(), None::<&str>);
                }
                tauri::webview::NewWindowResponse::Deny
            })
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
