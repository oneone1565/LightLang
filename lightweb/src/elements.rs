// LightLang 前端元素：把 页面 代码块里的元素语句渲染成 HTML
//
// 元素语法（Light 侧）：
//   按钮 "提交" 动作="/api" 方式=post 样式=主要
//   输入框 名称="名字" 占位提示="请输入"
//   表格 表头=["名称", "值"] 行=[["a", "1"]]
//   容器 元素="form" 动作="/api" 动作方式=post
//     输入框 名称="名字"
//     按钮 "提交"
//   结束
//
// 编译器按顺序调用 light_page_element_begin / _text / _attr / _end / _close，
// 这里负责累积元素并在结束时输出 HTML。

use std::collections::HashMap;

/// 元素种类（与 lightc 侧的元素编号一一对应）
pub const KIND_HEADING: i32 = 0;
pub const KIND_PARAGRAPH: i32 = 1;
pub const KIND_BUTTON: i32 = 2;
pub const KIND_LINK: i32 = 3;
pub const KIND_IMAGE: i32 = 4;
pub const KIND_INPUT: i32 = 5;
pub const KIND_TEXTAREA: i32 = 6;
pub const KIND_CHECKBOX: i32 = 7;
pub const KIND_RADIO: i32 = 8;
pub const KIND_SELECT: i32 = 9;
pub const KIND_LIST: i32 = 10;
pub const KIND_TABLE: i32 = 11;
pub const KIND_DIVIDER: i32 = 12;
pub const KIND_CARD: i32 = 13;
pub const KIND_BADGE: i32 = 14;
pub const KIND_PROGRESS: i32 = 15;
pub const KIND_CODE: i32 = 16;
pub const KIND_ALERT: i32 = 17;
pub const KIND_CONTAINER: i32 = 18;

/// 属性值：标量保存解码后的文本，容器保留结构编码
#[derive(Debug, Clone)]
pub enum AttrValue {
    Text(String),
    Tree(String),
}

impl AttrValue {
    pub fn from_encoded(encoded: &str) -> AttrValue {
        match decode_tree(encoded) {
            Node::Text(value) => AttrValue::Text(value),
            _ => AttrValue::Tree(encoded.to_string()),
        }
    }
}

#[derive(Default, Clone)]
pub struct Element {
    pub kind: i32,
    pub text: Option<String>,
    pub attrs: Vec<(String, AttrValue)>,
}

impl Element {
    fn entry(&self, name: &str) -> Option<&AttrValue> {
        self.attrs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    /// 纯文本属性：标量直接给文本，容器给展开后的可读文本
    pub fn attr(&self, name: &str) -> Option<String> {
        match self.entry(name)? {
            AttrValue::Text(value) => Some(value.clone()),
            AttrValue::Tree(encoded) => Some(flatten(&decode_tree(encoded))),
        }
    }

    /// 结构化属性：列表、表格等使用
    pub fn attr_tree(&self, name: &str) -> Option<&str> {
        match self.entry(name)? {
            AttrValue::Tree(encoded) => Some(encoded.as_str()),
            AttrValue::Text(value) => Some(value.as_str()),
        }
    }

    pub fn text_or(&self, fallback: &str) -> String {
        match &self.text {
            Some(value) => scalar_text(value).unwrap_or_else(|| fallback.to_string()),
            None => fallback.to_string(),
        }
    }

    pub fn flag(&self, name: &str) -> bool {
        matches!(self.attr(name).as_deref(), Some("真") | Some("true") | Some("1"))
    }
}

// ---------------------------------------------------------------------------
// 结构化值解码：s<长度>:<内容> / l<数量>:<元素…> / t / d
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Node {
    Text(String),
    List(Vec<Node>),
    Pairs(Vec<(String, Node)>),
}

pub fn decode_tree(encoded: &str) -> Node {
    let bytes = encoded.as_bytes();
    let mut cursor = 0usize;
    let node = decode_node(bytes, &mut cursor);
    node.unwrap_or(Node::Text(String::new()))
}

fn decode_node(bytes: &[u8], cursor: &mut usize) -> Option<Node> {
    if *cursor >= bytes.len() {
        return Some(Node::Text(String::new()));
    }
    let tag = bytes[*cursor];
    *cursor += 1;
    let mut digits = String::new();
    while *cursor < bytes.len() && bytes[*cursor] != b':' {
        digits.push(bytes[*cursor] as char);
        *cursor += 1;
    }
    *cursor += 1; // 跳过 ':'
    let count: usize = digits.trim().parse().unwrap_or(0);
    match tag {
        b'l' | b't' => {
            let mut items = Vec::with_capacity(count);
            for _ in 0..count {
                items.push(decode_node(bytes, cursor)?);
            }
            Some(Node::List(items))
        }
        b'd' => {
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let key = match decode_node(bytes, cursor)? {
                    Node::Text(value) => value,
                    other => format!("{:?}", other),
                };
                let value = decode_node(bytes, cursor)?;
                pairs.push((key, value));
            }
            Some(Node::Pairs(pairs))
        }
        _ => {
            let end = (*cursor + count).min(bytes.len());
            let text = String::from_utf8_lossy(&bytes[*cursor..end]).to_string();
            *cursor = end;
            Some(Node::Text(text))
        }
    }
}

/// 取出标量文本；容器则返回 None
pub fn scalar_text(encoded: &str) -> Option<String> {
    match decode_tree(encoded) {
        Node::Text(value) => Some(value),
        _ => None,
    }
}

/// 取出列表项；标量视为单项列表
pub fn list_items(encoded: &str) -> Vec<String> {
    match decode_tree(encoded) {
        Node::List(items) => items
            .iter()
            .map(|item| match item {
                Node::Text(value) => value.clone(),
                other => flatten(other),
            })
            .collect(),
        Node::Text(value) => vec![value],
        Node::Pairs(pairs) => pairs
            .iter()
            .map(|(key, value)| format!("{}: {}", key, flatten(value)))
            .collect(),
    }
}

/// 取出表格行（二维）
pub fn table_rows(encoded: &str) -> Vec<Vec<String>> {
    match decode_tree(encoded) {
        Node::List(items) => items
            .iter()
            .map(|item| match item {
                Node::List(cells) => cells
                    .iter()
                    .map(|cell| match cell {
                        Node::Text(value) => value.clone(),
                        other => flatten(other),
                    })
                    .collect(),
                Node::Text(value) => vec![value.clone()],
                other => vec![flatten(other)],
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn flatten(node: &Node) -> String {
    match node {
        Node::Text(value) => value.clone(),
        Node::List(items) => items
            .iter()
            .map(flatten)
            .collect::<Vec<String>>()
            .join(", "),
        Node::Pairs(pairs) => pairs
            .iter()
            .map(|(key, value)| format!("{}={}", key, flatten(value)))
            .collect::<Vec<String>>()
            .join(", "),
    }
}

// ---------------------------------------------------------------------------
// 渲染
// ---------------------------------------------------------------------------

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn number(value: Option<&str>, fallback: f64) -> f64 {
    value
        .and_then(|text| text.trim().parse::<f64>().ok())
        .unwrap_or(fallback)
}

fn number_attr(value: Option<&str>) -> String {
    match value {
        Some(text) if !text.trim().is_empty() => format!(" {}", escape(text)),
        _ => String::new(),
    }
}

pub fn render_element(element: &Element) -> String {
    match element.kind {
        KIND_HEADING => {
            let level = number(element.attr("级别").as_deref(), 2.0).clamp(1.0, 6.0) as i32;
            format!(
                "<h{level} class=\"lw-heading\">{text}</h{level}>",
                level = level,
                text = escape(&element.text_or(""))
            )
        }
        KIND_PARAGRAPH => format!(
            "<p class=\"lw-text\">{text}</p>",
            text = escape(&element.text_or(""))
        ),
        KIND_BUTTON => {
            let text = escape(&element.text_or("按钮"));
            let variant = escape(&element.attr("样式").unwrap_or_else(|| "主要".to_string()));
            let button = format!(
                "<button class=\"lw-btn lw-btn-{variant}\" type=\"submit\">{text}</button>",
                variant = variant,
                text = text
            );
            match element.attr("动作") {
                Some(action) if !action.is_empty() => {
                    let method = escape(&element.attr("方式").unwrap_or_else(|| "get".to_string())).to_lowercase();
                    let method = if method == "post" { "post" } else { "get" };
                    format!(
                        "<form class=\"lw-form\" method=\"{method}\" action=\"{action}\">{button}</form>",
                        method = method,
                        action = escape(&action),
                        button = button
                    )
                }
                _ => format!(
                    "<button class=\"lw-btn lw-btn-{variant}\" type=\"submit\">{text}</button>",
                    variant = variant,
                    text = text
                ),
            }
        }
        KIND_LINK => format!(
            "<a class=\"lw-link\" href=\"{href}\" rel=\"noopener\">{text}</a>",
            href = escape(&element.attr("地址").unwrap_or_else(|| "#".to_string())),
            text = escape(&element.text_or("链接"))
        ),
        KIND_IMAGE => format!(
            "<img class=\"lw-img\" src=\"{src}\" alt=\"{alt}\"{width}{height}>",
            src = escape(&element.attr("地址").unwrap_or_else(|| "".to_string())),
            alt = escape(&element.attr("替代文本").unwrap_or_else(|| "".to_string())),
            width = number_attr(element.attr("宽度").as_deref()),
            height = number_attr(element.attr("高度").as_deref())
        ),
        KIND_INPUT => {
            let input = format!(
                "<input class=\"lw-input\" type=\"{kind}\" name=\"{name}\" value=\"{value}\" placeholder=\"{hint}\">",
                kind = escape(&element.attr("类型").unwrap_or_else(|| "text".to_string())),
                name = escape(&element.attr("名称").unwrap_or_else(|| "".to_string())),
                value = escape(&element.attr("默认值").unwrap_or_else(|| "".to_string())),
                hint = escape(&element.attr("占位提示").unwrap_or_else(|| "".to_string()))
            );
            wrap_field(element, input)
        }
        KIND_TEXTAREA => {
            let input = format!(
                "<textarea class=\"lw-input lw-textarea\" name=\"{name}\" rows=\"{rows}\" placeholder=\"{hint}\">{value}</textarea>",
                name = escape(&element.attr("名称").unwrap_or_else(|| "".to_string())),
                rows = number(element.attr("行数").as_deref(), 4.0).clamp(2.0, 40.0) as i32,
                hint = escape(&element.attr("占位提示").unwrap_or_else(|| "".to_string())),
                value = escape(&element.attr("默认值").unwrap_or_else(|| "".to_string()))
            );
            wrap_field(element, input)
        }
        KIND_CHECKBOX | KIND_RADIO => {
            let kind = if element.kind == KIND_CHECKBOX {
                "checkbox"
            } else {
                "radio"
            };
            let checked = if element.flag("选中") { " checked" } else { "" };
            format!(
                "<label class=\"lw-check\"><input type=\"{kind}\" name=\"{name}\" value=\"{value}\"{checked}><span>{text}</span></label>",
                kind = kind,
                name = escape(&element.attr("名称").unwrap_or_else(|| "".to_string())),
                value = escape(&element.attr("值").unwrap_or_else(|| "".to_string())),
                checked = checked,
                text = escape(
                    &element
                        .attr("文字")
                        .unwrap_or_else(|| element.text_or(""))
                )
            )
        }
        KIND_SELECT => {
            let name = escape(&element.attr("名称").unwrap_or_else(|| "".to_string()));
            let mut options = String::new();
            if let Some(items) = element.attr_tree("选项") {
                for item in list_items(items) {
                    let selected = if element.attr("选中").as_deref() == Some(item.as_str()) {
                        " selected"
                    } else {
                        ""
                    };
                    options.push_str(&format!(
                        "<option value=\"{value}\"{selected}>{text}</option>",
                        value = escape(&item),
                        selected = selected,
                        text = escape(&item)
                    ));
                }
            }
            wrap_field(
                element,
                format!(
                    "<select class=\"lw-input\" name=\"{name}\">{options}</select>",
                    name = name,
                    options = options
                ),
            )
        }
        KIND_LIST => {
            let tag = if element.flag("有序") { "ol" } else { "ul" };
            let mut items = String::new();
            if let Some(encoded) = element.attr_tree("项目") {
                for item in list_items(encoded) {
                    items.push_str(&format!("<li>{}</li>", escape(&item)));
                }
            }
            format!("<{tag} class=\"lw-list\">{items}</{tag}>", tag = tag, items = items)
        }
        KIND_TABLE => {
            let mut head = String::new();
            if let Some(encoded) = element.attr_tree("表头") {
                let cells = list_items(encoded);
                head = format!(
                    "<thead><tr>{}</tr></thead>",
                    cells
                        .iter()
                        .map(|cell| format!("<th>{}</th>", escape(cell)))
                        .collect::<String>()
                );
            }
            let mut body = String::new();
            if let Some(encoded) = element.attr_tree("行") {
                let rows = table_rows(encoded);
                body = format!(
                    "<tbody>{}</tbody>",
                    rows.iter()
                        .map(|row| format!(
                            "<tr>{}</tr>",
                            row.iter()
                                .map(|cell| format!("<td>{}</td>", escape(cell)))
                                .collect::<String>()
                        ))
                        .collect::<String>()
                );
            }
            format!(
                "<div class=\"lw-table-wrap\"><table class=\"lw-table\">{head}{body}</table></div>",
                head = head,
                body = body
            )
        }
        KIND_DIVIDER => "<hr class=\"lw-hr\">".to_string(),
        KIND_CARD => {
            let mut html = String::from("<div class=\"lw-card\">");
            if let Some(title) = element.attr("标题") {
                html.push_str(&format!("<h3 class=\"lw-card-title\">{}</h3>", escape(&title)));
            }
            let body = element
                .attr("内容")
                .map(|value| escape(&value))
                .unwrap_or_else(|| escape(&element.text_or("")));
            html.push_str(&format!("<div class=\"lw-card-body\">{}</div>", body));
            html.push_str("</div>");
            html
        }
        KIND_BADGE => {
            let color = escape(&element.attr("颜色").unwrap_or_else(|| "蓝".to_string()));
            format!(
                "<span class=\"lw-badge lw-badge-{color}\">{text}</span>",
                color = color,
                text = escape(&element.text_or("标签"))
            )
        }
        KIND_PROGRESS => {
            let max = number(element.attr("最大").as_deref(), 100.0).max(1.0);
            let value = number(element.attr("值").as_deref(), 0.0).clamp(0.0, max);
            let percent = (value / max) * 100.0;
            format!(
                "<div class=\"lw-progress-wrap\"><div class=\"lw-progress\"><div class=\"lw-progress-bar\" style=\"width:{percent:.1}%\"></div></div><span class=\"lw-progress-text\">{value} / {max}</span></div>",
                percent = percent,
                value = escape(&trim_number(value)),
                max = escape(&trim_number(max))
            )
        }
        KIND_CODE => format!(
            "<pre class=\"lw-code\"><code>{text}</code></pre>",
            text = escape(&element.text_or(""))
        ),
        KIND_ALERT => {
            let level = escape(&element.attr("级别").unwrap_or_else(|| "信息".to_string()));
            format!(
                "<div class=\"lw-alert lw-alert-{level}\">{text}</div>",
                level = level,
                text = escape(&element.text_or("提示"))
            )
        }
        KIND_CONTAINER => {
            let tag = container_tag(element);
            let mut html = format!("<{} class=\"lw-container", tag);
            if let Some(class) = element.attr("类名") {
                html.push_str(&format!(" {}", escape(&class)));
            }
            html.push('"');
            if let Some(id) = element.attr("名称") {
                html.push_str(&format!(" id=\"{}\"", escape(&id)));
            }
            // 容器也可以是表单：容器 元素=form 动作="/save" 方式=post
            if tag == "form" {
                let method = escape(
                    &element.attr("方式").unwrap_or_else(|| "get".to_string()),
                )
                .to_lowercase();
                let method = if method == "post" { "post" } else { "get" };
                html.push_str(&format!(" method=\"{}\"", method));
                if let Some(action) = element.attr("动作") {
                    html.push_str(&format!(" action=\"{}\"", escape(&action)));
                }
            }
            html.push('>');
            if let Some(title) = element.attr("标题") {
                html.push_str(&format!(
                    "<h2 class=\"lw-section-title\">{}</h2>",
                    escape(&title)
                ));
            }
            html
        }
        _ => String::new(),
    }
}

fn trim_number(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{}", value as i64)
    } else {
        format!("{}", value)
    }
}

/// 容器使用的 HTML 标签（只允许安全标签）
pub fn container_tag(element: &Element) -> String {
    sanitize_tag(&element.attr("元素").unwrap_or_else(|| "div".to_string()))
}

fn sanitize_tag(tag: &str) -> String {
    const ALLOWED: [&str; 8] = [
        "div", "section", "form", "fieldset", "article", "nav", "header", "footer",
    ];
    let lowered = tag.trim().to_lowercase();
    if ALLOWED.contains(&lowered.as_str()) {
        lowered
    } else {
        "div".to_string()
    }
}

fn wrap_field(element: &Element, control: String) -> String {
    match element.attr("标签") {
        Some(label) if !label.is_empty() => format!(
            "<label class=\"lw-field\"><span class=\"lw-label\">{}</span>{}</label>",
            escape(&label),
            control
        ),
        _ => format!("<div class=\"lw-field\">{}</div>", control),
    }
}

/// 前端基础样式：亮色/暗色主题都可用
pub fn base_css(dark: bool) -> String {
    let (bg, fg, muted, border, surface, accent, accent_fg) = if dark {
        (
            "#101418",
            "#e6edf3",
            "#9aa7b4",
            "#243040",
            "#161c24",
            "#4c9aff",
            "#0b1017",
        )
    } else {
        (
            "#ffffff",
            "#111827",
            "#6b7280",
            "#e5e7eb",
            "#f9fafb",
            "#2563eb",
            "#ffffff",
        )
    };
    format!(
        r#"body {{ margin: 0; padding: 2rem; background: {bg}; color: {fg}; font-family: system-ui, "Microsoft YaHei", sans-serif; line-height: 1.6; }}
.lw-container {{ margin: 0 0 1rem; }}
.lw-section-title {{ margin: 0 0 .75rem; font-size: 1.25rem; }}
.lw-heading {{ margin: 1.25rem 0 .5rem; }}
.lw-text {{ margin: .5rem 0; }}
.lw-form {{ display: inline-block; margin: 0 .5rem .5rem 0; }}
.lw-btn {{ display: inline-block; padding: .5rem 1.1rem; border: 1px solid {accent}; border-radius: 8px; background: {accent}; color: {accent_fg}; font-size: .95rem; cursor: pointer; text-decoration: none; }}
.lw-btn:hover {{ filter: brightness(1.08); }}
.lw-btn-次要, .lw-btn-次要:hover {{ background: transparent; color: {fg}; border-color: {border}; }}
.lw-btn-危险 {{ background: #d93025; border-color: #d93025; color: #fff; }}
.lw-btn-链接 {{ background: transparent; border-color: transparent; color: {accent}; }}
.lw-link {{ color: {accent}; }}
.lw-img {{ max-width: 100%; border-radius: 8px; }}
.lw-field {{ display: flex; flex-direction: column; gap: .3rem; margin: 0 0 .75rem; max-width: 32rem; }}
.lw-label {{ color: {muted}; font-size: .85rem; }}
.lw-input {{ padding: .45rem .6rem; border: 1px solid {border}; border-radius: 8px; background: {surface}; color: {fg}; font-size: .95rem; font-family: inherit; }}
.lw-textarea {{ min-height: 5rem; resize: vertical; }}
.lw-check {{ display: inline-flex; align-items: center; gap: .4rem; margin: 0 1rem .5rem 0; }}
.lw-list {{ margin: .5rem 0; padding-left: 1.4rem; }}
.lw-table-wrap {{ overflow-x: auto; margin: .75rem 0; }}
.lw-table {{ border-collapse: collapse; width: 100%; }}
.lw-table th, .lw-table td {{ border: 1px solid {border}; padding: .45rem .7rem; text-align: left; }}
.lw-table th {{ background: {surface}; }}
.lw-hr {{ border: none; border-top: 1px solid {border}; margin: 1.25rem 0; }}
.lw-card {{ border: 1px solid {border}; border-radius: 10px; background: {surface}; padding: 1rem; margin: 0 0 .75rem; }}
.lw-card-title {{ margin: 0 0 .4rem; font-size: 1.05rem; }}
.lw-card-body {{ color: {fg}; }}
.lw-badge {{ display: inline-block; padding: .1rem .55rem; border-radius: 999px; background: {accent}; color: {accent_fg}; font-size: .8rem; margin: 0 .35rem .35rem 0; }}
.lw-badge-蓝, .lw-badge-主要 {{ background: {accent}; color: {accent_fg}; }}
.lw-badge-绿, .lw-badge-成功 {{ background: #1f9d55; color: #fff; }}
.lw-badge-黄, .lw-badge-警告 {{ background: #d99e0b; color: #1c1917; }}
.lw-badge-红, .lw-badge-危险 {{ background: #d93025; color: #fff; }}
.lw-badge-灰 {{ background: {border}; color: {fg}; }}
.lw-progress-wrap {{ display: flex; align-items: center; gap: .6rem; margin: .5rem 0; max-width: 32rem; }}
.lw-progress {{ flex: 1; height: .55rem; background: {border}; border-radius: 999px; overflow: hidden; }}
.lw-progress-bar {{ height: 100%; background: {accent}; }}
.lw-progress-text {{ color: {muted}; font-size: .85rem; white-space: nowrap; }}
.lw-code {{ background: {surface}; border: 1px solid {border}; border-radius: 8px; padding: .8rem; overflow-x: auto; }}
.lw-alert {{ border: 1px solid {border}; border-left-width: 4px; border-radius: 8px; padding: .6rem .9rem; margin: .6rem 0; max-width: 40rem; }}
.lw-alert-成功 {{ border-left-color: #1f9d55; }}
.lw-alert-警告 {{ border-left-color: #d99e0b; }}
.lw-alert-错误, .lw-alert-危险 {{ border-left-color: #d93025; }}
.lw-alert-信息 {{ border-left-color: {accent}; }}"#,
        bg = bg,
        fg = fg,
        muted = muted,
        border = border,
        surface = surface,
        accent = accent,
        accent_fg = accent_fg
    )
}

/// 元素名 -> 种类编号（编译器与运行时的唯一对应表）
pub fn kind_by_name(name: &str) -> Option<i32> {
    let table: HashMap<&str, i32> = HashMap::from([
        ("标题", KIND_HEADING),
        ("段落", KIND_PARAGRAPH),
        ("按钮", KIND_BUTTON),
        ("链接", KIND_LINK),
        ("图片", KIND_IMAGE),
        ("输入框", KIND_INPUT),
        ("文本域", KIND_TEXTAREA),
        ("复选框", KIND_CHECKBOX),
        ("单选框", KIND_RADIO),
        ("下拉框", KIND_SELECT),
        ("列表", KIND_LIST),
        ("表格", KIND_TABLE),
        ("分隔线", KIND_DIVIDER),
        ("卡片", KIND_CARD),
        ("标签", KIND_BADGE),
        ("进度条", KIND_PROGRESS),
        ("代码块", KIND_CODE),
        ("提示", KIND_ALERT),
        ("容器", KIND_CONTAINER),
    ]);
    table.get(name).copied()
}

/// 全部元素名，供编译器校验
pub fn element_names() -> Vec<&'static str> {
    vec![
        "标题",
        "段落",
        "按钮",
        "链接",
        "图片",
        "输入框",
        "文本域",
        "复选框",
        "单选框",
        "下拉框",
        "列表",
        "表格",
        "分隔线",
        "卡片",
        "标签",
        "进度条",
        "代码块",
        "提示",
        "容器",
        "结束",
    ]
}
