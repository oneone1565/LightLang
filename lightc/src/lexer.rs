//! Light 词法分析器
//!
//! 支持中文关键字、Python 风格缩进块、UTF-8 标识符与字符串。
//! 缩进用空格或 Tab（不可混用）；块以冒号 `:` 结束行 + 缩进表示。

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // 关键字
    Let,      // 让
    Print,    // 打印
    Throw,    // 抛出
    Thread,   // 线程
    Page,     // 页面
    Try,      // 尝试
    Catch,    // 捕获
    Finally,  // 最终
    If,       // 如果
    Elif,     // 否则如果
    Else,     // 否则
    Fn,       // 函数
    Return,   // 返回
    While,    // 当
    For,      // 对于
    In,       // 在
    Break,    // 跳出
    Continue, // 继续
    True,     // 真
    False,    // 假
    None,     // 空
    Array,    // 数组
    Asm,      // 汇编
    Extern,   // 外部
    Import,   // 导入
    // 逻辑运算符
    And,      // 且
    Or,       // 或
    Not,      // 非
    // 字面量
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    // 运算符
    Plus,
    Minus,
    Star,
    StarStar, // **
    Slash,
    SlashSlash, // //
    Percent,
    Assign,      // =
    Eq,          // ==
    Neq,         // !=
    Lt,          // <
    Gt,          // >
    Le,          // <=
    Ge,          // >=
    // 标点
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    // 结构
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub _col: usize,
}

impl fmt::Display for TokenKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenKind::Let => write!(f, "让"),
            TokenKind::Print => write!(f, "打印"),
            TokenKind::Throw => write!(f, "抛出"),
            TokenKind::Thread => write!(f, "线程"),
            TokenKind::Page => write!(f, "页面"),
            TokenKind::Try => write!(f, "尝试"),
            TokenKind::Catch => write!(f, "捕获"),
            TokenKind::Finally => write!(f, "最终"),
            TokenKind::If => write!(f, "如果"),
            TokenKind::Elif => write!(f, "否则如果"),
            TokenKind::Else => write!(f, "否则"),
            TokenKind::Fn => write!(f, "函数"),
            TokenKind::Return => write!(f, "返回"),
            TokenKind::While => write!(f, "当"),
            TokenKind::For => write!(f, "对于"),
            TokenKind::In => write!(f, "在"),
            TokenKind::Break => write!(f, "跳出"),
            TokenKind::Continue => write!(f, "继续"),
            TokenKind::True => write!(f, "真"),
            TokenKind::False => write!(f, "假"),
            TokenKind::None => write!(f, "空"),
            TokenKind::Array => write!(f, "数组"),
            TokenKind::Asm => write!(f, "汇编"),
            TokenKind::Extern => write!(f, "外部"),
            TokenKind::Import => write!(f, "导入"),
            TokenKind::And => write!(f, "且"),
            TokenKind::Or => write!(f, "或"),
            TokenKind::Not => write!(f, "非"),
            TokenKind::Int(n) => write!(f, "{}", n),
            TokenKind::Float(n) => write!(f, "{}", n),
            TokenKind::Str(s) => write!(f, "{:?}", s),
            TokenKind::Ident(s) => write!(f, "{}", s),
            TokenKind::Plus => write!(f, "+"),
            TokenKind::Minus => write!(f, "-"),
            TokenKind::Star => write!(f, "*"),
            TokenKind::StarStar => write!(f, "**"),
            TokenKind::Slash => write!(f, "/"),
            TokenKind::SlashSlash => write!(f, "//"),
            TokenKind::Percent => write!(f, "%"),
            TokenKind::Assign => write!(f, "="),
            TokenKind::Eq => write!(f, "=="),
            TokenKind::Neq => write!(f, "!="),
            TokenKind::Lt => write!(f, "<"),
            TokenKind::Gt => write!(f, ">"),
            TokenKind::Le => write!(f, "<="),
            TokenKind::Ge => write!(f, ">="),
            TokenKind::LParen => write!(f, "("),
            TokenKind::RParen => write!(f, ")"),
            TokenKind::LBracket => write!(f, "["),
            TokenKind::RBracket => write!(f, "]"),
            TokenKind::LBrace => write!(f, "{{"),
            TokenKind::RBrace => write!(f, "}}"),
            TokenKind::Comma => write!(f, ","),
            TokenKind::Colon => write!(f, ":"),
            TokenKind::Dot => write!(f, "."),
            TokenKind::Newline => write!(f, "换行"),
            TokenKind::Indent => write!(f, "缩进"),
            TokenKind::Dedent => write!(f, "取消缩进"),
            TokenKind::Eof => write!(f, "结束"),
        }
    }
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    indent_stack: Vec<usize>,
    pending: Vec<Token>,
    at_line_start: bool,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
            line: 1,
            col: 1,
            indent_stack: vec![0],
            pending: Vec::new(),
            at_line_start: true,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.chars.get(self.pos).copied()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn make(&self, kind: TokenKind) -> Token {
        Token { kind, line: self.line, _col: self.col }
    }

    /// 跳过注释（# 开头到行尾）和空白（非行首）
    fn skip_inline_ws_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\r') => { self.bump(); }
                Some('#') => {
                    while let Some(c) = self.peek() {
                        if c == '\n' { break; }
                        self.bump();
                    }
                }
                _ => break,
            }
        }
    }

    fn is_ident_start(c: char) -> bool {
        c.is_alphabetic() || c == '_'
    }

    fn is_ident_cont(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    fn read_ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.peek() {
            if Self::is_ident_cont(c) {
                s.push(c);
                self.bump();
            } else {
                break;
            }
        }
        s
    }

    fn keyword(s: &str) -> Option<TokenKind> {
        match s {
            "让" => Some(TokenKind::Let),
            "打印" => Some(TokenKind::Print),
            "抛出" => Some(TokenKind::Throw),
            "线程" => Some(TokenKind::Thread),
            "页面" => Some(TokenKind::Page),
            "尝试" => Some(TokenKind::Try),
            "捕获" => Some(TokenKind::Catch),
            "最终" => Some(TokenKind::Finally),
            "如果" => Some(TokenKind::If),
            "否则如果" => Some(TokenKind::Elif),
            "否则" => Some(TokenKind::Else),
            "函数" => Some(TokenKind::Fn),
            "返回" => Some(TokenKind::Return),
            "当" => Some(TokenKind::While),
            "对于" => Some(TokenKind::For),
            "在" => Some(TokenKind::In),
            "跳出" => Some(TokenKind::Break),
            "继续" => Some(TokenKind::Continue),
            "真" => Some(TokenKind::True),
            "假" => Some(TokenKind::False),
            "空" => Some(TokenKind::None),
            "数组" => Some(TokenKind::Array),
            "汇编" => Some(TokenKind::Asm),
            "外部" => Some(TokenKind::Extern),
            "导入" => Some(TokenKind::Import),
            "且" => Some(TokenKind::And),
            "或" => Some(TokenKind::Or),
            "非" => Some(TokenKind::Not),
            _ => None,
        }
    }

    /// 读取数字，返回整数或浮点（用 Result 区分）
    fn read_number(&mut self) -> TokenKind {
        let mut s = String::new();
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                s.push(c);
                self.bump();
            } else if c == '.' && !is_float {
                // 确保下一个字符是数字（避免与方法调用/范围混淆）
                if self.peek_at_n(1).map_or(false, |c| c.is_ascii_digit()) {
                    is_float = true;
                    s.push(c);
                    self.bump();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        if is_float {
            TokenKind::Float(s.parse().unwrap_or(0.0))
        } else {
            TokenKind::Int(s.parse().unwrap_or(0))
        }
    }

    fn peek_at_n(&self, off: usize) -> Option<char> {
        self.chars.get(self.pos + off).copied()
    }

    fn read_string(&mut self) -> Result<String, String> {
        let quote = self.bump().unwrap(); // " or '
        let mut s = String::new();
        loop {
            match self.bump() {
                None => return Err("字符串未闭合".into()),
                Some(c) if c == quote => break,
                Some('\\') => {
                    match self.bump() {
                        Some('n') => s.push('\n'),
                        Some('t') => s.push('\t'),
                        Some('\\') => s.push('\\'),
                        Some('"') => s.push('"'),
                        Some('\'') => s.push('\''),
                        Some('0') => s.push('\0'),
                        Some(other) => { s.push('\\'); s.push(other); }
                        None => return Err("转义序列未完成".into()),
                    }
                }
                Some(c) => s.push(c),
            }
        }
        Ok(s)
    }

    fn next_raw(&mut self) -> Result<Token, String> {
        loop {
            // 行首：处理缩进
            if self.at_line_start {
                self.at_line_start = false;
                let mut indent = 0usize;
                let mut used_tab = false;
                let mut used_space = false;
                loop {
                    match self.peek() {
                        Some(' ') => { indent += 1; used_space = true; self.bump(); }
                        Some('\t') => { indent += 1; used_tab = true; self.bump(); }
                        _ => break,
                    }
                }
                // 空行或纯注释行：忽略
                match self.peek() {
                    Some('\n') | Some('\r') => {
                        // 跳过换行，继续行首处理
                        while let Some(c) = self.peek() {
                            if c == '\n' || c == '\r' { self.bump(); } else { break; }
                        }
                        self.at_line_start = true;
                        continue;
                    }
                    Some('#') => {
                        while let Some(c) = self.peek() {
                            if c == '\n' { break; }
                            self.bump();
                        }
                        while let Some(c) = self.peek() {
                            if c == '\n' || c == '\r' { self.bump(); } else { break; }
                        }
                        self.at_line_start = true;
                        continue;
                    }
                    None => {
                        // EOF：发出剩余 Dedent
                        while self.indent_stack.len() > 1 {
                            self.indent_stack.pop();
                            self.pending.push(self.make(TokenKind::Dedent));
                        }
                        return Ok(self.make(TokenKind::Eof));
                    }
                    _ => {}
                }
                if used_tab && used_space {
                    return Err(format!("第 {} 行：Tab 与空格不可混用", self.line));
                }
                let current = *self.indent_stack.last().unwrap();
                if indent > current {
                    self.indent_stack.push(indent);
                    return Ok(self.make(TokenKind::Indent));
                } else if indent < current {
                    let mut toks = Vec::new();
                    while *self.indent_stack.last().unwrap() > indent {
                        self.indent_stack.pop();
                        toks.push(self.make(TokenKind::Dedent));
                    }
                    if *self.indent_stack.last().unwrap() != indent {
                        return Err(format!("第 {} 行：缩进级别不匹配", self.line));
                    }
                    // 返回第一个 Dedent，其余放入 pending
                    let first = toks.remove(0);
                    for t in toks {
                        self.pending.push(t);
                    }
                    return Ok(first);
                }
                // indent == current：继续正常 tokenize
            }

            self.skip_inline_ws_and_comments();

            let line = self.line;
            let col = self.col;
            let c = match self.peek() {
                Some(c) => c,
                None => {
                    // EOF：发出剩余 Dedent
                    while self.indent_stack.len() > 1 {
                        self.indent_stack.pop();
                        self.pending.push(Token { kind: TokenKind::Dedent, line, _col: col });
                    }
                    return Ok(Token { kind: TokenKind::Eof, line, _col: col });
                }
            };

            let tok = match c {
                '\n' | '\r' => {
                    while let Some(ch) = self.peek() {
                        if ch == '\n' || ch == '\r' { self.bump(); } else { break; }
                    }
                    self.at_line_start = true;
                    Token { kind: TokenKind::Newline, line, _col: col }
                }
                '+' => { self.bump(); Token { kind: TokenKind::Plus, line, _col: col } }
                '-' => { self.bump(); Token { kind: TokenKind::Minus, line, _col: col } }
                '*' => {
                    self.bump();
                    if self.peek() == Some('*') {
                        self.bump();
                        Token { kind: TokenKind::StarStar, line, _col: col }
                    } else {
                        Token { kind: TokenKind::Star, line, _col: col }
                    }
                }
                '/' => {
                    self.bump();
                    if self.peek() == Some('/') {
                        self.bump();
                        Token { kind: TokenKind::SlashSlash, line, _col: col }
                    } else {
                        Token { kind: TokenKind::Slash, line, _col: col }
                    }
                }
                '%' => { self.bump(); Token { kind: TokenKind::Percent, line, _col: col } }
                '=' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        Token { kind: TokenKind::Eq, line, _col: col }
                    } else {
                        Token { kind: TokenKind::Assign, line, _col: col }
                    }
                }
                '!' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        Token { kind: TokenKind::Neq, line, _col: col }
                    } else {
                        return Err(format!("第 {} 行：意外字符 '!'", line));
                    }
                }
                '<' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        Token { kind: TokenKind::Le, line, _col: col }
                    } else {
                        Token { kind: TokenKind::Lt, line, _col: col }
                    }
                }
                '>' => {
                    self.bump();
                    if self.peek() == Some('=') {
                        self.bump();
                        Token { kind: TokenKind::Ge, line, _col: col }
                    } else {
                        Token { kind: TokenKind::Gt, line, _col: col }
                    }
                }
                '(' => { self.bump(); Token { kind: TokenKind::LParen, line, _col: col } }
                ')' => { self.bump(); Token { kind: TokenKind::RParen, line, _col: col } }
                '[' => { self.bump(); Token { kind: TokenKind::LBracket, line, _col: col } }
                ']' => { self.bump(); Token { kind: TokenKind::RBracket, line, _col: col } }
                '{' => { self.bump(); Token { kind: TokenKind::LBrace, line, _col: col } }
                '}' => { self.bump(); Token { kind: TokenKind::RBrace, line, _col: col } }
                ',' => { self.bump(); Token { kind: TokenKind::Comma, line, _col: col } }
                ':' => { self.bump(); Token { kind: TokenKind::Colon, line, _col: col } }
                '.' => { self.bump(); Token { kind: TokenKind::Dot, line, _col: col } }
                '"' | '\'' => {
                    let s = self.read_string()
                        .map_err(|e| format!("第 {} 行：{}", line, e))?;
                    Token { kind: TokenKind::Str(s), line, _col: col }
                }
                c if c.is_ascii_digit() => {
                    let kind = self.read_number();
                    Token { kind, line, _col: col }
                }
                c if Self::is_ident_start(c) => {
                    let s = self.read_ident();
                    let kind = Self::keyword(&s).unwrap_or(TokenKind::Ident(s));
                    Token { kind, line, _col: col }
                }
                other => {
                    return Err(format!("第 {} 行：无法识别的字符 '{}'", line, other));
                }
            };
            return Ok(tok);
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, String> {
        let mut out = Vec::new();
        loop {
            if let Some(t) = self.pending.pop() {
                out.push(t);
                continue;
            }
            let t = self.next_raw()?;
            let is_eof = matches!(t.kind, TokenKind::Eof);
            out.push(t);
            if is_eof { break; }
        }
        Ok(out)
    }
}
