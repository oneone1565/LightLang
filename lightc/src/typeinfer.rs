//! Light 类型推断（单态、不动点迭代）
//!
//! 推断每个函数的参数类型与返回类型，用于：
//!   - 数组索引、字符串拼接的类型分派
//!   - RAII：确定哪些参数/变量拥有堆资源需释放
//!
//! 策略：约束收集 + 跨函数传播
//!   1. 收集本函数内 let 的类型、return 的类型、调用点实参的类型
//!   2. 调用点把实参类型传播给被调者形参；本函数把 return 表达式类型传播为返回类型
//!   3. 反复迭代直到不再变化（不动点），使类型跨函数传播

use std::collections::HashMap;

use crate::ast::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Array,
    Tuple(Vec<Ty>),
    List,
    Dict,
    None,
    /// 未知/待定（用于参数未定时的占位）
    Unknown,
}

#[derive(Default, Clone)]
pub struct TypeEnv {
    /// (函数名, 参数索引) -> 类型
    pub param_types: HashMap<(String, usize), Ty>,
    /// 函数名 -> 返回类型
    pub return_types: HashMap<String, Ty>,
    /// 函数名 -> 参数名列表（供分析时把标识符解析到参数类型）
    pub param_names: HashMap<String, Vec<String>>,
}

/// 单次遍历中收集到的约束
#[derive(Default)]
struct Constraints {
    params: Vec<(String, usize, Ty)>,
    rets: Vec<(String, Ty)>,
}

/// 对整个程序做不动点类型推断
pub fn infer(program: &Program) -> TypeEnv {
    let mut env = TypeEnv::default();

    // 收集所有函数名与参数名
    let mut fns: Vec<&FnDef> = Vec::new();
    for item in &program.items {
        if let Item::Fn(f) = item {
            fns.push(f);
            env.param_names.insert(f.name.clone(), f.params.clone());
            for (i, _) in f.params.iter().enumerate() {
                env.param_types.insert((f.name.clone(), i), Ty::Unknown);
            }
            env.return_types.insert(f.name.clone(), Ty::Int);
        }
    }

    // 不动点迭代
    for _ in 0..50 {
        let mut changed = false;
        let mut cons = Constraints::default();

        for f in &fns {
            analyze_fn(f, &env, &mut cons);
        }

        let mut top_locals = HashMap::new();
        for item in &program.items {
            if let Item::Stmt(stmt) = item {
                collect_locals_stmt(stmt, &mut top_locals, &env, "<top>");
            }
        }
        for item in &program.items {
            if let Item::Stmt(stmt) = item {
                analyze_stmt(stmt, "<top>", &top_locals, &env, &mut cons);
            }
        }

        // 应用参数约束（传播实参类型到形参）
        for (fname, idx, ty) in &cons.params {
            if apply_param(&mut env, fname, *idx, ty.clone()) {
                changed = true;
            }
        }

        // 应用返回类型约束
        for (fname, ty) in &cons.rets {
            if apply_ret(&mut env, fname, ty.clone()) {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    // 收尾：Unknown 兜底为 Int
    for v in env.param_types.values_mut() {
        if *v == Ty::Unknown {
            *v = Ty::Int;
        }
    }
    for v in env.return_types.values_mut() {
        if *v == Ty::Unknown {
            *v = Ty::Int;
        }
    }

    env
}

fn apply_param(env: &mut TypeEnv, fname: &str, idx: usize, ty: Ty) -> bool {
    let key = (fname.to_string(), idx);
    let cur = env.param_types.get(&key).cloned().unwrap_or(Ty::Unknown);
    if cur == ty {
        return false;
    }
    if cur == Ty::Unknown {
        env.param_types.insert(key, ty);
        return true;
    }
    // 已有类型：允许升级（Int 被更强类型覆盖）
    if matches!(cur, Ty::Int) && !matches!(ty, Ty::Int) {
        env.param_types.insert(key, ty);
        return true;
    }
    false
}

fn apply_ret(env: &mut TypeEnv, name: &str, ty: Ty) -> bool {
    if matches!(ty, Ty::Unknown) {
        return false;
    }
    let cur = env.return_types.get(name).cloned().unwrap_or(Ty::Int);
    if cur == ty {
        return false;
    }
    // 返回类型可从 Int 升级为具体类型
    if matches!(cur, Ty::Int) && !matches!(ty, Ty::Int) {
        env.return_types.insert(name.to_string(), ty);
        return true;
    }
    false
}

/// 把标识符按名字解析为类型：优先局部变量，其次当前函数参数
fn resolve_ident(name: &str, locals: &HashMap<String, Ty>,
                  env: &TypeEnv, fname: &str) -> Option<Ty> {
    if let Some(t) = locals.get(name) {
        return Some(t.clone());
    }
    let names = env.param_names.get(fname)?;
    let idx = names.iter().position(|n| n == name)?;
    env.param_types.get(&(fname.to_string(), idx)).cloned()
}

/// 分析单个函数，收集参数/返回约束
fn analyze_fn(f: &FnDef, env: &TypeEnv, cons: &mut Constraints) {
    let mut locals: HashMap<String, Ty> = HashMap::new();
    collect_locals(&f.body, &mut locals, env, &f.name);

    for stmt in &f.body.stmts {
        analyze_stmt(stmt, &f.name, &locals, env, cons);
    }
}

fn collect_locals(block: &Block, locals: &mut HashMap<String, Ty>, env: &TypeEnv, fname: &str) {
    for stmt in &block.stmts {
        collect_locals_stmt(stmt, locals, env, fname);
    }
}

fn collect_locals_stmt(stmt: &Stmt, locals: &mut HashMap<String, Ty>, env: &TypeEnv, fname: &str) {
    match stmt {
        Stmt::Let { name, value } => {
            let ty = expr_type(value, locals, env, fname);
            locals.insert(name.clone(), ty);
        }
        Stmt::Assign { target, value } => {
            if let Expr::Ident(name) = target {
                let ty = expr_type(value, locals, env, fname);
                locals.insert(name.clone(), ty);
            }
        }
        Stmt::If { then, elifs, els, .. } => {
            collect_locals(then, locals, env, fname);
            for (_, b) in elifs {
                collect_locals(b, locals, env, fname);
            }
            if let Some(b) = els {
                collect_locals(b, locals, env, fname);
            }
        }
        Stmt::While { body, .. } => collect_locals(body, locals, env, fname),
        Stmt::For { body, .. } => collect_locals(body, locals, env, fname),
        _ => {}
    }
}

fn analyze_stmt(stmt: &Stmt, fname: &str, locals: &HashMap<String, Ty>,
                env: &TypeEnv, cons: &mut Constraints) {
    match stmt {
        Stmt::Let { value, .. } => analyze_expr(value, fname, locals, env, cons),
        Stmt::Assign { target, value } => {
            analyze_expr(target, fname, locals, env, cons);
            analyze_expr(value, fname, locals, env, cons);
        }
        Stmt::Expr(e) | Stmt::Print(e) => analyze_expr(e, fname, locals, env, cons),
        Stmt::If { cond, then, elifs, els, .. } => {
            analyze_expr(cond, fname, locals, env, cons);
            for s in &then.stmts { analyze_stmt(s, fname, locals, env, cons); }
            for (c, b) in elifs {
                analyze_expr(c, fname, locals, env, cons);
                for s in &b.stmts { analyze_stmt(s, fname, locals, env, cons); }
            }
            if let Some(b) = els {
                for s in &b.stmts { analyze_stmt(s, fname, locals, env, cons); }
            }
        }
        Stmt::While { cond, body, .. } => {
            analyze_expr(cond, fname, locals, env, cons);
            for s in &body.stmts { analyze_stmt(s, fname, locals, env, cons); }
        }
        Stmt::For { iter, body, .. } => {
            analyze_expr(iter, fname, locals, env, cons);
            for s in &body.stmts { analyze_stmt(s, fname, locals, env, cons); }
        }
        Stmt::Return(Some(e)) => {
            let t = expr_type(e, locals, env, fname);
            cons.rets.push((fname.to_string(), t));
            analyze_expr(e, fname, locals, env, cons);
        }
        Stmt::Return(None) => {
            cons.rets.push((fname.to_string(), Ty::None));
        }
        _ => {}
    }
}

/// 如果 expr 是当前函数参数，则给它附加类型约束（用于字符串/数组上下文推断）
fn constrain_arg(expr: &Expr, want: Ty, fname: &str, env: &TypeEnv, cons: &mut Constraints) {
    if let Expr::Ident(name) = expr {
        if want != Ty::Unknown {
            if let Some(idx) = env_param_index(fname, name, env) {
                cons.params.push((fname.to_string(), idx, want));
            }
        }
    }
}

fn env_param_index(fname: &str, name: &str, env: &TypeEnv) -> Option<usize> {
    env.param_names.get(fname)?.iter().position(|n| n == name)
}

fn analyze_expr(expr: &Expr, fname: &str, locals: &HashMap<String, Ty>,
                 env: &TypeEnv, cons: &mut Constraints) {
    match expr {
        Expr::Ident(_) | Expr::Int(_) | Expr::Float(_) | Expr::Bool(_)
        | Expr::None | Expr::Str(_) => {}
        Expr::Tuple(items) | Expr::List(items) => {
            for item in items {
                analyze_expr(item, fname, locals, env, cons);
            }
        }
        Expr::Dict(entries) => {
            for (k, v) in entries {
                analyze_expr(k, fname, locals, env, cons);
                analyze_expr(v, fname, locals, env, cons);
            }
        }
        Expr::BinOp { op, lhs, rhs } => {
            use BinOp::*;
            if matches!(op, Add | Eq | Neq) {
                // 字符串拼接/比较：一边已知为 Str 时，把另一边的参数约束为 Str
                let lt = expr_type(lhs, locals, env, fname);
                let rt = expr_type(rhs, locals, env, fname);
                if lt == Ty::Str {
                    constrain_arg(rhs, Ty::Str, fname, env, cons);
                }
                if rt == Ty::Str {
                    constrain_arg(lhs, Ty::Str, fname, env, cons);
                }
                // 数组连接（Add）一边为 Array 时约束另一边
                if matches!(op, Add) && lt == Ty::Array {
                    constrain_arg(rhs, Ty::Array, fname, env, cons);
                }
                if matches!(op, Add) && rt == Ty::Array {
                    constrain_arg(lhs, Ty::Array, fname, env, cons);
                }
            }
            analyze_expr(lhs, fname, locals, env, cons);
            analyze_expr(rhs, fname, locals, env, cons);
        }
        Expr::UnaryOp { operand, .. } => analyze_expr(operand, fname, locals, env, cons),
        Expr::Call { name, args } => {
            let name = name.rsplit('.').next().unwrap_or(name.as_str());
            // 内置函数对特定参数有类型要求时，直接约束形参
            binop_builtin_hint(name, args, locals, env, fname, cons);
            let mut arg_tys = Vec::with_capacity(args.len());
            for a in args {
                arg_tys.push(expr_type(a, locals, env, fname));
                analyze_expr(a, fname, locals, env, cons);
            }
            // 把实参类型传播给被调函数形参（内置函数映射由 hint 覆盖，这里跳过）
            if !is_builtin(name) {
                for (i, t) in arg_tys.into_iter().enumerate() {
                    if matches!(t, Ty::Unknown) {
                        continue;
                    }
                    cons.params.push((name.to_string(), i, t));
                }
            }
        }
        Expr::Index { arr, idx } => {
            analyze_expr(arr, fname, locals, env, cons);
            analyze_expr(idx, fname, locals, env, cons);
        }
        Expr::ArrayNew(e) => analyze_expr(e, fname, locals, env, cons),
    }
}

fn is_builtin(name: &str) -> bool {
    matches!(name,
        "范围" | "长度" | "绝对值" | "最小" | "最大" | "求和" | "整数" | "浮点"
        | "字符串" | "输入" | "追加" | "弹出"
        | "正弦" | "余弦" | "正切" | "平方根" | "对数" | "对数10" | "幂"
        | "圆周率" | "自然常数" | "向上取整" | "向下取整" | "随机" | "随机整数"
        | "查找" | "包含" | "替换" | "分割" | "合并" | "大写" | "小写" | "去空白" | "切片"
        | "读取文件" | "写入文件" | "追加文件" | "文件存在"
        | "执行" | "环境变量" | "退出" | "字符"
        | "反转" | "索引" | "排序" | "连接" | "数组")
}

/// 内置函数对字符串/数组参数的隐含类型要求
fn binop_builtin_hint(name: &str, args: &[Expr], locals: &HashMap<String, Ty>,
                      env: &TypeEnv, fname: &str, cons: &mut Constraints) {
    if args.is_empty() {
        return;
    }
    // 字符串上下文内置函数：第一个参数应为字符串
    let str_hint = matches!(name,
        "查找" | "替换" | "大写" | "小写" | "去空白" |
        "读取文件" | "执行" | "环境变量" | "分割");
    // 数组上下文内置函数
    let array_hint = matches!(name,
        "求和" | "反转" | "排序" | "追加" | "弹出" | "索引" | "合并" | "连接" | "长度");
    // 混合上下文：包含/切片既能接收 字符串 也能数组，需要依另一参数判断
    if str_hint {
        constrain_arg(&args[0], Ty::Str, fname, env, cons);
    } else if array_hint {
        constrain_arg(&args[0], Ty::Array, fname, env, cons);
        if name == "连接" && args.len() > 1 {
            constrain_arg(&args[1], Ty::Array, fname, env, cons);
        }
    } else if name == "包含" {
        let second = args.get(1).map(|a| expr_type(a, locals, env, fname)).unwrap_or(Ty::Unknown);
        if second == Ty::Str {
            constrain_arg(&args[0], Ty::Str, fname, env, cons);
        } else {
            // 第二实参最新类型：如果是参数则无法立刻判断，交由后续迭代
            constrain_arg(&args[0], Ty::Array, fname, env, cons);
        }
    } else if name == "切片" {
        // 第一个参数：优先字符串（更常见）
        constrain_arg(&args[0], Ty::Str, fname, env, cons);
    }
}

/// 推断表达式类型（只读）
pub fn expr_type(expr: &Expr, locals: &HashMap<String, Ty>, env: &TypeEnv, fname: &str) -> Ty {
    match expr {
        Expr::Int(_) => Ty::Int,
        Expr::Float(_) => Ty::Float,
        Expr::Bool(_) => Ty::Bool,
        Expr::None => Ty::None,
        Expr::Str(_) => Ty::Str,
        Expr::Ident(name) => {
            if let Some(t) = resolve_ident(name, locals, env, fname) {
                return t;
            }
            Ty::Int
        }
        Expr::Tuple(items) => {
            let mut tys = Vec::new();
            for item in items {
                tys.push(expr_type(item, locals, env, fname));
            }
            Ty::Tuple(tys)
        }
        Expr::List(items) => {
            if items.is_empty() {
                return Ty::List;
            }
            Ty::List
        }
        Expr::Dict(_) => {
            Ty::Dict
        }
        Expr::BinOp { op, lhs, rhs } => {
            use BinOp::*;
            let lt = expr_type(lhs, locals, env, fname);
            let rt = expr_type(rhs, locals, env, fname);
            match op {
                Add | Sub | Mul => {
                    if lt == Ty::Str || rt == Ty::Str { Ty::Str }
                    else if lt == Ty::Float || rt == Ty::Float { Ty::Float }
                    else { Ty::Int }
                }
                Div => {
                    if lt == Ty::Str || rt == Ty::Str { Ty::Str }
                    else { Ty::Float }
                }
                FloorDiv | Mod | Pow => {
                    if lt == Ty::Float || rt == Ty::Float { Ty::Float } else { Ty::Int }
                }
                Eq | Neq | Lt | Gt | Le | Ge | And | Or => Ty::Bool,
            }
        }
        Expr::UnaryOp { op, operand } => {
            match op {
                UnaryOp::Neg => {
                    let t = expr_type(operand, locals, env, fname);
                    if t == Ty::Float { Ty::Float } else { Ty::Int }
                }
                UnaryOp::Not => Ty::Bool,
            }
        }
        Expr::Call { name, args } => {
            let name = name.rsplit('.').next().unwrap_or(name.as_str());
            match name {
                "数组" => return Ty::Array,
                "范围" => return Ty::Array,
                "长度" => return Ty::Int,
                "绝对值" => {
                    if let Some(a) = args.first() {
                        return expr_type(a, locals, env, fname);
                    }
                    return Ty::Int;
                }
                "最小" | "最大" | "求和" => return Ty::Int,
                "整数" => return Ty::Int,
                "浮点" => return Ty::Float,
                "字符串" => return Ty::Str,
                "输入" => return Ty::Str,
                "追加" | "弹出" => return Ty::None,
                "正弦" | "余弦" | "正切" | "平方根" | "对数" | "对数10"
                | "幂" | "圆周率" | "自然常数" | "向上取整" | "向下取整"
                | "随机" => return Ty::Float,
                "随机整数" => return Ty::Int,
                "查找" => return Ty::Int,
                "包含" | "文件存在" => return Ty::Bool,
                "替换" | "大写" | "小写" | "去空白" | "切片" => return Ty::Str,
                "分割" => return Ty::Array,
                "合并" | "读取文件" | "执行" | "环境变量" => return Ty::Str,
                "写入文件" | "追加文件" => return Ty::Bool,
                "反转" | "连接" => return Ty::Array,
                "索引" => return Ty::Int,
                "排序" => return Ty::None,
                "退出" => return Ty::None,
                "字符" => return Ty::Str,
                _ => {}
            }
            if let Some(t) = env.return_types.get(name) {
                return t.clone();
            }
            Ty::Int
        }
        Expr::Index { arr, idx: _ } => {
            let at = expr_type(arr, locals, env, fname);
            let _ = at;
            // 数组索引 → 元素为 i64；字符串索引 → 字节 int
            Ty::Int
        }
        Expr::ArrayNew(_) => Ty::Array,
    }
}