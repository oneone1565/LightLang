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
/// 内建英文别名表：源文件头写 `语言 : 英文` 时装载。
/// 键是英文写法，值是 Light 原名。
pub const ENGLISH: &[(&str, &str)] = &[
    // 关键字
    ("let", "让"), ("print", "打印"), ("throw", "抛出"), ("thread", "线程"),
    ("page", "页面"), ("assert", "断言"), ("try", "尝试"), ("catch", "捕获"),
    ("finally", "最终"), ("if", "如果"), ("elif", "否则如果"), ("else", "否则"),
    ("fn", "函数"), ("return", "返回"), ("while", "当"), ("for", "对于"),
    ("in", "在"), ("break", "跳出"), ("continue", "继续"),
    ("true", "真"), ("false", "假"), ("none", "空"), ("array", "数组"),
    ("asm", "汇编"), ("extern", "外部"), ("import", "导入"),
    ("and", "且"), ("or", "或"), ("not", "非"),
    // 类型名
    ("int", "整数"), ("float", "浮点"), ("string", "字符串"), ("bool", "布尔"),
    ("list", "列表"), ("tuple", "元组"), ("dict", "字典"),
    // 通用
    ("len", "长度"), ("contains", "包含"), ("find", "查找"), ("index_of", "索引"),
    ("sum", "求和"), ("min", "最小"), ("max", "最大"), ("abs", "绝对值"),
    ("range", "范围"), ("sort", "排序"), ("reverse", "反转"),
    ("input", "输入"), ("exit", "退出"),
    // 类型转换
    ("int_of", "整数"), ("float_of", "浮点"), ("str_of", "字符串"),
    ("char_at", "字符"),
    // 字符串
    ("upper", "大写"), ("lower", "小写"), ("trim", "去空白"),
    ("slice", "切片"), ("split", "分割"), ("join", "合并"),
    ("replace", "替换"), ("concat_str", "连接"),
    // 容器
    ("append", "追加"), ("pop", "弹出"), ("push", "追加"),
    ("exists_key", "存在"),
    // 文件与系统
    ("read_file", "读取文件"), ("write_file", "写入文件"),
    ("append_file", "追加文件"), ("file_exists", "文件存在"),
    ("getenv", "环境变量"), ("exec", "执行"), ("argv", "命令行参数"),
    // 数学
    ("sin", "正弦"), ("cos", "余弦"), ("tan", "正切"), ("sqrt", "平方根"),
    ("log", "对数"), ("log10", "对数10"), ("pow", "幂"),
    ("floor", "向下取整"), ("ceil", "向上取整"),
    ("random", "随机"), ("random_int", "随机整数"),
    ("pi", "圆周率"), ("e", "自然常数"),
    // 诊断
    ("eprint", "错误输出"),
];

/// 装载内建英文别名，返回条数
pub fn load_english() -> usize {
    if let Ok(mut t) = table().write() {
        for (en, zh) in ENGLISH {
            t.insert((*en).to_string(), (*zh).to_string());
        }
        ENGLISH.len()
    } else {
        0
    }
}
