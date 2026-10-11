//! 用户自定义别名：`.llang` 文件里声明「某个原名也能用别的写法」。
//!
//! 例：`print : 打印` 之后，源码里写 `print "hi"` 与 `打印 "hi"` 等价。
//! 其余语义完全不变——别名只在词法阶段生效，不影响运行期表示。
//!
//! 预扫描阶段就把 `.llang` 里的声明装进这张表，词法器识别关键字前先查一次，
//! 这样关键字类内建（打印/真/假/函数…）也能用别名。

use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};

fn table() -> &'static RwLock<HashMap<String, String>> {
    static TABLE: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    TABLE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// 把用户写的名字还原成原名；没有别名就原样返回。
pub fn resolve(name: &str) -> String {
    match table().read() {
        Ok(t) => t.get(name).cloned().unwrap_or_else(|| name.to_string()),
        Err(_) => name.to_string(),
    }
}

/// 当前登记了多少条别名；为 0 时词法器可以完全跳过查表。
pub fn count() -> usize {
    table().read().map(|t| t.len()).unwrap_or(0)
}

/// 解析 `.llang`：每行一条 `别名 : 原名`，`#` 开头是注释。返回读到的条数。
pub fn load_llang(source: &str) -> Result<usize, String> {
    let mut count = 0usize;
    for (lineno, raw) in source.lines().enumerate() {
        let line = match raw.find('#') {
            Some(i) => &raw[..i],
            None => raw,
        }
        .trim();
        if line.is_empty() {
            continue;
        }
        let Some((alias, canonical)) = line.split_once(':') else {
            return Err(format!(
                "别名需要写成「别名 : 原名」（第 {} 行：{}）",
                lineno + 1,
                line
            ));
        };
        let (alias, canonical) = (alias.trim(), canonical.trim());
        if alias.is_empty() || canonical.is_empty() {
            return Err(format!("别名两侧都不能为空（第 {} 行）", lineno + 1));
        }
        if let Ok(mut t) = table().write() {
            t.insert(alias.to_string(), canonical.to_string());
        }
        count += 1;
    }
    Ok(count)
}