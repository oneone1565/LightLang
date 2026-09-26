//! Light 语法分析器（递归下降）

use crate::ast::*;
use crate::lexer::{Token, TokenKind};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    fn expect(&mut self, kind: TokenKind) -> Result<Token, String> {
        if self.peek_kind() == &kind {
            Ok(self.bump())
        } else {
            Err(format!(
                "第 {} 行：期望 {}，得到 {}",
                self.peek().line, kind, self.peek().kind
            ))
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek_kind(), TokenKind::Newline) {
            self.bump();
        }
    }

    pub fn parse_program(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();
        self.skip_newlines();
        while !matches!(self.peek_kind(), TokenKind::Eof) {
            items.push(self.parse_item()?);
            self.skip_newlines();
        }
        Ok(Program { items })
    }

    fn parse_item(&mut self) -> Result<Item, String> {
        match self.peek_kind() {
            TokenKind::Fn => Ok(Item::Fn(self.parse_fn()?)),
            TokenKind::Extern => Ok(Item::Extern(self.parse_extern()?)),
            TokenKind::Ident(name) if name == "从" => Ok(Item::Import(self.parse_from_import()?)),
            TokenKind::Import => Ok(Item::Import(self.parse_import()?)),
            _ => Ok(Item::Stmt(self.parse_stmt()?)),
        }
    }

    fn parse_fn(&mut self) -> Result<FnDef, String> {
        self.expect(TokenKind::Fn)?;
        let name = match self.bump().kind {
            TokenKind::Ident(s) => s,
            other => return Err(format!("函数名必须是标识符，得到 {}", other)),
        };
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                match self.bump().kind {
                    TokenKind::Ident(s) => params.push(s),
                    other => return Err(format!("参数名必须是标识符，得到 {}", other)),
                }
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Colon)?;
        self.expect(TokenKind::Newline)?;
        let body = self.parse_block()?;
        Ok(FnDef { name, params, body })
    }

    fn parse_extern(&mut self) -> Result<ExternDecl, String> {
        self.expect(TokenKind::Extern)?;
        let name = match self.bump().kind {
            TokenKind::Ident(s) => s,
            other => return Err(format!("外部函数名必须是标识符，得到 {}", other)),
        };
        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                match self.bump().kind {
                    TokenKind::Ident(s) => params.push(s),
                    other => return Err(format!("参数名必须是标识符，得到 {}", other)),
                }
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Newline)?;
        Ok(ExternDecl { name, params })
    }

    fn parse_import(&mut self) -> Result<String, String> {
        self.expect(TokenKind::Import)?;
        let path = match self.bump().kind {
            TokenKind::Str(s) => s,
            other => return Err(format!("导入路径必须是字符串，得到 {}", other)),
        };
        self.consume_line_end()?;
        Ok(path)
    }

    fn parse_from_import(&mut self) -> Result<String, String> {
        self.expect(TokenKind::Ident("从".to_string()))?;
        let path = match self.bump().kind {
            TokenKind::Str(s) => s,
            other => return Err(format!("导入路径必须是字符串，得到 {}", other)),
        };
        self.expect(TokenKind::Import)?;
        loop {
            match self.bump().kind {
                TokenKind::Ident(_) => {}
                other => return Err(format!("导入名称必须是标识符，得到 {}", other)),
            }
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    continue;
                } else {
                    break;
                }
        }
        self.consume_line_end()?;
        Ok(path)
    }

    fn parse_block(&mut self) -> Result<Block, String> {
        self.expect(TokenKind::Indent)?;
        let mut stmts = Vec::new();
        loop {
            if matches!(self.peek_kind(), TokenKind::Dedent | TokenKind::Eof) {
                break;
            }
            stmts.push(self.parse_stmt()?);
            self.skip_newlines();
        }
        if matches!(self.peek_kind(), TokenKind::Dedent) {
            self.bump();
        }
        Ok(Block { stmts })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, String> {
        match self.peek_kind().clone() {
            TokenKind::Let => {
                self.bump();
                let name = match self.bump().kind {
                    TokenKind::Ident(s) => s,
                    other => return Err(format!("变量名必须是标识符，得到 {}", other)),
                };
                self.expect(TokenKind::Assign)?;
                let value = self.parse_expr()?;
                self.consume_line_end()?;
                Ok(Stmt::Let { name, value })
            }
            TokenKind::Print => {
                self.bump();
                let value = self.parse_expr()?;
                self.consume_line_end()?;
                Ok(Stmt::Print(value))
            }
            TokenKind::Throw => {
                self.bump();
                let value = self.parse_expr()?;
                self.consume_line_end()?;
                Ok(Stmt::Throw(value))
            }
            TokenKind::Thread => self.parse_thread(),
            TokenKind::Page => self.parse_page(),
            TokenKind::Try => self.parse_try(),
            TokenKind::If => self.parse_if(),
            TokenKind::While => {
                self.bump();
                let cond = self.parse_expr()?;
                self.expect(TokenKind::Colon)?;
                self.expect(TokenKind::Newline)?;
                let body = self.parse_block()?;
                Ok(Stmt::While { cond, body })
            }
            TokenKind::For => {
                self.bump();
                let var = match self.bump().kind {
                    TokenKind::Ident(s) => s,
                    other => return Err(format!("循环变量必须是标识符，得到 {}", other)),
                };
                self.expect(TokenKind::In)?;
                let iter = self.parse_expr()?;
                self.expect(TokenKind::Colon)?;
                self.expect(TokenKind::Newline)?;
                let body = self.parse_block()?;
                Ok(Stmt::For { var, iter, body })
            }
            TokenKind::Break => {
                self.bump();
                self.consume_line_end()?;
                Ok(Stmt::Break)
            }
            TokenKind::Continue => {
                self.bump();
                self.consume_line_end()?;
                Ok(Stmt::Continue)
            }
            TokenKind::Return => {
                self.bump();
                let value = if matches!(self.peek_kind(), TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.consume_line_end()?;
                Ok(Stmt::Return(value))
            }
            TokenKind::Asm => {
                self.bump();
                let raw = match self.bump().kind {
                    TokenKind::Str(s) => s,
                    other => return Err(format!("汇编块需要字符串字面量，得到 {}", other)),
                };
                self.consume_line_end()?;
                Ok(Stmt::Asm(raw))
            }
            _ => {
                let expr = self.parse_expr()?;
                if matches!(self.peek_kind(), TokenKind::Assign) {
                    self.bump();
                    let value = self.parse_expr()?;
                    self.consume_line_end()?;
                    Ok(Stmt::Assign { target: expr, value })
                } else {
                    self.consume_line_end()?;
                    Ok(Stmt::Expr(expr))
                }
            }
        }
    }

    fn parse_page(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Page)?;
        self.expect(TokenKind::LParen)?;
        let mut title = "LightLang".to_string();
        let mut icon = "none".to_string();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                let key = match self.bump().kind {
                    TokenKind::Ident(key) => key,
                    other => return Err(format!("页面属性名必须是标识符，得到 {}", other)),
                };
                self.expect(TokenKind::Assign)?;
                let value = match self.bump().kind {
                    TokenKind::Str(value) => value,
                    TokenKind::Ident(value) => value,
                    other => return Err(format!("页面属性值必须是字符串或标识符，得到 {}", other)),
                };
                match key.as_str() {
                    "标题" | "title" => title = value,
                    "图标" | "icon" => icon = value,
                    other => return Err(format!("未知页面属性：{}", other)),
                }
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else if matches!(self.peek_kind(), TokenKind::Ident(_)) {
                    continue;
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Colon)?;
        self.expect(TokenKind::Newline)?;
        let body = self.parse_block()?;
        Ok(Stmt::Page { title, icon, body })
    }

    fn parse_thread(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Thread)?;
        self.expect(TokenKind::LParen)?;
        let id = match self.bump().kind {
            TokenKind::Int(id) => id,
            other => return Err(format!("线程编号必须是整数，得到 {}", other)),
        };
        self.expect(TokenKind::RParen)?;
        self.expect(TokenKind::Colon)?;
        self.expect(TokenKind::Newline)?;
        let body = self.parse_block()?;
        Ok(Stmt::Thread { id, body })
    }

    fn parse_try(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::Try)?;
        self.expect(TokenKind::Colon)?;
        self.expect(TokenKind::Newline)?;
        let body = self.parse_block()?;
        let mut catch = None;
        let mut finally = None;
        if matches!(self.peek_kind(), TokenKind::Catch) {
            self.bump();
            let name = match self.bump().kind {
                TokenKind::Ident(name) => name,
                other => return Err(format!("捕获变量必须是标识符，得到 {}", other)),
            };
            self.expect(TokenKind::Colon)?;
            self.expect(TokenKind::Newline)?;
            catch = Some((name, self.parse_block()?));
        }
        if matches!(self.peek_kind(), TokenKind::Finally) {
            self.bump();
            self.expect(TokenKind::Colon)?;
            self.expect(TokenKind::Newline)?;
            finally = Some(self.parse_block()?);
        }
        Ok(Stmt::Try { body, catch, finally })
    }

    fn parse_if(&mut self) -> Result<Stmt, String> {
        self.expect(TokenKind::If)?;
        let cond = self.parse_expr()?;
        self.expect(TokenKind::Colon)?;
        self.expect(TokenKind::Newline)?;
        let then = self.parse_block()?;
        let mut elifs = Vec::new();
        let mut els = None;
        self.skip_newlines();
        loop {
            match self.peek_kind() {
                TokenKind::Elif => {
                    self.bump();
                    let ec = self.parse_expr()?;
                    self.expect(TokenKind::Colon)?;
                    self.expect(TokenKind::Newline)?;
                    let eb = self.parse_block()?;
                    elifs.push((ec, eb));
                    self.skip_newlines();
                }
                TokenKind::Else => {
                    self.bump();
                    self.expect(TokenKind::Colon)?;
                    self.expect(TokenKind::Newline)?;
                    els = Some(self.parse_block()?);
                    break;
                }
                _ => break,
            }
        }
        Ok(Stmt::If { cond, then, elifs, els })
    }

    fn consume_line_end(&mut self) -> Result<(), String> {
        match self.peek_kind() {
            TokenKind::Newline | TokenKind::Dedent | TokenKind::Eof => {
                if matches!(self.peek_kind(), TokenKind::Newline) {
                    self.bump();
                }
                Ok(())
            }
            other => Err(format!("第 {} 行：期望换行，得到 {}", self.peek().line, other)),
        }
    }

    // ---- 表达式优先级（从低到高）----
    // or < and < not(unary) < 比较 < 加减 < 乘除 < 幂 < 一元(-) < 后缀(调用/索引) < 原子

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_and()?;
        loop {
            if !matches!(self.peek_kind(), TokenKind::Or) { break; }
            self.bump();
            let rhs = self.parse_and()?;
            lhs = Expr::BinOp { op: BinOp::Or, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_not()?;
        loop {
            if !matches!(self.peek_kind(), TokenKind::And) { break; }
            self.bump();
            let rhs = self.parse_not()?;
            lhs = Expr::BinOp { op: BinOp::And, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_not(&mut self) -> Result<Expr, String> {
        if matches!(self.peek_kind(), TokenKind::Not) {
            self.bump();
            let operand = self.parse_not()?;
            Ok(Expr::UnaryOp { op: UnaryOp::Not, operand: Box::new(operand) })
        } else {
            self.parse_cmp()
        }
    }

    fn parse_cmp(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Eq => BinOp::Eq,
                TokenKind::Neq => BinOp::Neq,
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_add()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_mul()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, String> {
        let mut lhs = self.parse_pow()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::SlashSlash => BinOp::FloorDiv,
                TokenKind::Percent => BinOp::Mod,
                _ => break,
            };
            self.bump();
            let rhs = self.parse_pow()?;
            lhs = Expr::BinOp { op, lhs: Box::new(lhs), rhs: Box::new(rhs) };
        }
        Ok(lhs)
    }

    fn parse_pow(&mut self) -> Result<Expr, String> {
        // 幂运算右结合
        let lhs = self.parse_unary()?;
        if matches!(self.peek_kind(), TokenKind::StarStar) {
            self.bump();
            let rhs = self.parse_pow()?;
            Ok(Expr::BinOp { op: BinOp::Pow, lhs: Box::new(lhs), rhs: Box::new(rhs) })
        } else {
            Ok(lhs)
        }
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek_kind(), TokenKind::Minus) {
            self.bump();
            let operand = self.parse_unary()?;
            Ok(Expr::UnaryOp { op: UnaryOp::Neg, operand: Box::new(operand) })
        } else {
            self.parse_postfix()
        }
    }

    fn is_string_method(name: &str) -> bool {
        matches!(name,
            "长度" | "大写" | "小写" | "去空白" | "查找" | "替换" | "分割" | "包含" | "切片"
            | "len" | "upper" | "lower" | "strip" | "find" | "replace" | "split" | "contains" | "slice")
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(TokenKind::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek_kind(), TokenKind::RParen) {
            loop {
                args.push(self.parse_expr()?);
                if matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump();
                } else {
                    break;
                }
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(args)
    }

    fn parse_postfix(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_atom()?;
        loop {
            match self.peek_kind() {
                TokenKind::Dot => {
                    self.bump();
                    let member = match self.bump().kind {
                        TokenKind::Ident(name) => name,
                        other => return Err(format!("模块成员必须是标识符，得到 {}", other)),
                    };
                    let name = match e {
                        Expr::Ident(name) => name,
                        _ => return Err("模块成员访问只能从标识符开始".into()),
                    };
                    if name.contains('.') || !Self::is_string_method(&member) {
                        e = Expr::Ident(format!("{}.{}", name, member));
                    } else {
                        let mut args = vec![Expr::Ident(name)];
                        args.extend(self.parse_call_args()?);
                        e = Expr::Call { name: member, args };
                    }
                }
                TokenKind::LParen => {
                    let args = self.parse_call_args()?;
                    if let Expr::Ident(name) = e {
                        e = Expr::Call { name, args };
                    } else {
                        return Err("只能调用标识符或模块函数".into());
                    }
                }
                TokenKind::LBracket => {
                    self.bump();
                    let idx = self.parse_expr()?;
                    self.expect(TokenKind::RBracket)?;
                    e = Expr::Index { arr: Box::new(e), idx: Box::new(idx) };
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        let t = self.bump();
        match t.kind {
            TokenKind::Int(n) => Ok(Expr::Int(n)),
            TokenKind::Float(n) => Ok(Expr::Float(n)),
            TokenKind::Str(s) => Ok(Expr::Str(s)),
            TokenKind::True => Ok(Expr::Bool(true)),
            TokenKind::False => Ok(Expr::Bool(false)),
            TokenKind::None => Ok(Expr::None),
            TokenKind::Ident(s) => Ok(Expr::Ident(s)),
            TokenKind::LParen => {
                let mut items = Vec::new();
                items.push(self.parse_expr()?);
                let mut has_comma = false;
                while matches!(self.peek_kind(), TokenKind::Comma) {
                    self.bump(); // consume comma
                    has_comma = true;
                    if matches!(self.peek_kind(), TokenKind::RParen) {
                        break; // trailing comma
                    }
                    items.push(self.parse_expr()?);
                }
                if matches!(self.peek_kind(), TokenKind::RParen) {
                    self.bump(); // consume RParen
                } else {
                    self.expect(TokenKind::RParen)?;
                }
                if has_comma {
                    Ok(Expr::Tuple(items))
                } else {
                    Ok(items.into_iter().next().unwrap())
                }
            }
            TokenKind::LBracket => {
                let mut items = Vec::new();
                if !matches!(self.peek_kind(), TokenKind::RBracket) {
                    loop {
                        items.push(self.parse_expr()?);
                        if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.bump();
                            if matches!(self.peek_kind(), TokenKind::RBracket) {
                                break; // 尾部逗号
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(Expr::List(items))
            }
            TokenKind::LBrace => {
                let mut entries = Vec::new();
                if !matches!(self.peek_kind(), TokenKind::RBrace) {
                    loop {
                        let key = self.parse_expr()?;
                        self.expect(TokenKind::Colon)?;
                        let val = self.parse_expr()?;
                        entries.push((key, val));
                        if matches!(self.peek_kind(), TokenKind::Comma) {
                            self.bump();
                            if matches!(self.peek_kind(), TokenKind::RBrace) {
                                break; // 尾部逗号
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Expr::Dict(entries))
            }
            TokenKind::Array => {
                self.expect(TokenKind::LParen)?;
                let len = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(Expr::ArrayNew(Box::new(len)))
            }
            other => Err(format!("第 {} 行：意外的 token {}", t.line, other)),
        }
    }
}
