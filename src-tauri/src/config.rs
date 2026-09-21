use std::path::Path;

use serde::{Deserialize, Serialize};

/// 一个可配置的标签页
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TabConfig {
    pub id: String,
    pub name: String,
    pub url: String,
}

/// 应用配置，持久化为 tabs.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 启动时自动打开配置列表的前 N 个标签
    pub startup_count: usize,
    pub tabs: Vec<TabConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            startup_count: 2,
            tabs: vec![
                TabConfig {
                    id: "t_deepseek".into(),
                    name: "DeepSeek".into(),
                    url: "https://chat.deepseek.com".into(),
                },
                TabConfig {
                    id: "t_kimi".into(),
                    name: "Kimi".into(),
                    url: "https://www.kimi.com".into(),
                },
            ],
        }
    }
}

impl AppConfig {
    /// 读取配置；文件不存在或格式错误时回退到默认配置
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }
}

/// 生成标签 id：时间戳 + 纳秒尾数，不引入额外依赖
pub fn new_tab_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap();
    format!("t{}_{:03}", now.as_millis(), now.as_nanos() % 1000)
}
