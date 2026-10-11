//! Light 抽象语法树

#[derive(Debug, Clone)]
pub struct Program {
    pub items: Vec<Item>,
}

#[derive(Debug, Clone)]
pub enum Item {
    Fn(FnDef),
    Extern(ExternDecl),
    Import(String),
    Stmt(Stmt),
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<String>,
    /// 参数类型标注：函数 参数: 列表 中的类型名（None 表示未标注）
    pub param_types: Vec<Option<String>>,
    /// 返回类型标注：函数 f() -> 字符串:
    pub return_type: Option<String>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct ExternDecl {
    pub name: String,
    pub params: Vec<String>,
    /// 每个形参的类型标注；None 表示动态（运行期靠标签分派）
    pub param_types: Vec<Option<String>>,
    /// 返回类型标注；None 表示动态
    pub return_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let { name: String, value: Expr },
    Assign { target: Expr, value: Expr },
    Expr(Expr),
    /// 打印：首项为格式串或值，其余为按位置填充的参数
    /// 支持 `打印("hi,{名字}")`（按作用域取值）与 `打印("hi,{0}", 名字)`（显式传参）
    Print(Vec<Expr>),
    Style(Expr),
    /// 页面元素：元素名 + 可选文本 + 属性键值对 + 可选子块（容器用）
    Element {
        kind: String,
        text: Option<Expr>,
        attrs: Vec<(String, Expr)>,
        body: Option<Block>,
    },
    /// 结束最近的页面容器
    ElementEnd,
    Assert { condition: Expr, message: Option<Expr> },
    Throw(Expr),
    Thread { id: i64, body: Block },
    Page { title: String, icon: String, body: Block },
    Try {
        body: Block,
        catch: Option<(String, Block)>,
        finally: Option<Block>,
    },
    /// if/elif/else 链：elifs 为 (条件, 块) 列表
    If { cond: Expr, then: Block, elifs: Vec<(Expr, Block)>, els: Option<Block> },
    While { cond: Expr, body: Block },
    /// 对于 变量 在 可迭代对象:  块
    For { var: String, iter: Expr, body: Block },
    Break,
    Continue,
    Return(Option<Expr>),
    /// 内联汇编
    Asm(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    None,
    Str(String),
    Ident(String),
    BinOp { op: BinOp, lhs: Box<Expr>, rhs: Box<Expr> },
    UnaryOp { op: UnaryOp, operand: Box<Expr> },
    Call { name: String, args: Vec<Expr> },
    Index { arr: Box<Expr>, idx: Box<Expr> },
    ArrayNew(Box<Expr>),
    /// 元组：(1, 2, 3)
    Tuple(Vec<Expr>),
    /// 列表：[1, 2, 3]
    List(Vec<Expr>),
    /// 字典：{"key": value, ...}
    Dict(Vec<(Expr, Expr)>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add, Sub, Mul, Div, FloorDiv, Mod, Pow,
    Eq, Neq, Lt, Gt, Le, Ge,
    And, Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}
