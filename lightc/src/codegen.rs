//! Light 代码生成器：输出 LLVM IR 文本，交由 clang/LLVM 编译为汇编/机器码。
//!
//! 内存安全模型（Rust 风格，无 GC）：
//!   - 所有运行时堆对象（字符串、数组）由 lightrt（Rust）管理
//!   - 编译器跟踪每个作用域内拥有堆资源的变量
//!   - 离开作用域时自动插入释放调用（RAII）
//!   - 重新赋值时先释放旧值
//!   - 作为参数传入函数时视为移动（所有权转移）
//!
//! 值表示：
//!   - 所有变量统一用 `alloca i64` 存储
//!   - 整数/布尔/指针直接是 i64
//!   - 浮点（double）通过 bitcast 与 i64 互相转换后存储
//!   - 函数参数与返回值统一为 i64，浮点在边界做 bitcast

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::typeinfer::{self, Ty as ITy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ty {
    Int,
    Float,
    Bool,
    Str,
    Array,
    Tuple,
    List,
    Dict,
    None,
}

impl From<ITy> for Ty {
    fn from(t: ITy) -> Self {
        match t {
            ITy::Int => Ty::Int,
            ITy::Float => Ty::Float,
            ITy::Bool => Ty::Bool,
            ITy::Str => Ty::Str,
            ITy::Array => Ty::Array,
            ITy::Tuple(_) => Ty::Tuple,
            ITy::List => Ty::List,
            ITy::Dict => Ty::Dict,
            ITy::None => Ty::None,
            ITy::Unknown => Ty::Int,
        }
    }
}

impl Ty {
    fn is_owned(self) -> bool {
        matches!(self, Ty::Str | Ty::Array | Ty::Dict | Ty::Tuple)
    }
    fn is_float(self) -> bool { matches!(self, Ty::Float) }
}

struct Scope {
    vars: HashMap<String, (String, Ty)>, // name -> (alloca reg, type)
    owned: Vec<String>,                   // 拥有堆资源、需在作用域结束时释放的变量
}

/// 循环上下文：break 与 continue 的目标标签
struct LoopCtx {
    brk: String,
    cont: String,
}

pub struct CodeGen<'a> {
    out: String,
    tmp: usize,
    str_idx: usize,
    globals: Vec<String>,
    externs: HashSet<String>,
    env: &'a typeinfer::TypeEnv,
    loops: Vec<LoopCtx>,
    deferred: Vec<String>,
    page_mode: bool,
    target_triple: String,
}

const RUNTIME_DECLS: &[&str] = &[
    "declare void @light_print_int(i64)",
    "declare void @light_print_float(double)",
    "declare void @light_print_str(ptr, i64)",
    "declare void @light_print_newline()",
    "declare void @light_print_bool(i64)",
    "declare void @light_print_value(i64, i32)",
    "declare void @light_print_array(ptr)",
    "declare void @light_print_tuple(ptr)",
    "declare void @light_print_dict(ptr)",
    "declare void @light_throw(ptr)",
    "declare ptr @light_take_error()",
    "declare i64 @light_web_register_v1(i32, ptr, i64, ptr)",
    "declare i64 @light_web_register_html_v1(i32, ptr, i64, ptr)",
    "declare i64 @light_web_run_v1(i64)",
    "declare i64 @light_web_request_method_v1(i64)",
    "declare i64 @light_web_request_path_v1(i64)",
    "declare i64 @light_web_request_body_v1(i64)",
    "declare i64 @light_web_request_header_v1(i64, ptr, i64)",
    "declare void @light_page_reset(ptr, i64, ptr, i64)",
    "declare void @light_page_set_lang(ptr, i64)",
    "declare void @light_page_set_theme(ptr, i64)",
    "declare void @light_page_append_str(ptr, i64)",
    "declare void @light_page_append_int(i64)",
    "declare void @light_page_append_float(double)",
    "declare void @light_page_append_bool(i64)",
    "declare ptr @light_page_finish()",
    "declare ptr @light_thread_spawn(ptr, i64)",
    "declare i64 @light_thread_join(ptr)",
    "declare i64 @light_thread_join_all()",
    "declare ptr @light_array_new(i64)",
    "declare i64 @light_array_get(ptr, i64)",
    "declare void @light_array_set(ptr, i64, i64)",
    "declare void @light_array_set_copy_tag(ptr, i64, i32, i64)",
    "declare i64 @light_array_len(ptr)",
    "declare void @light_array_free(ptr)",
    "declare void @light_array_push(ptr, i64)",
    "declare i64 @light_array_pop(ptr)",
    "declare ptr @light_dict_new()",
    "declare void @light_dict_set(ptr, i32, i64, i32, i64)",
    "declare void @light_dict_free(ptr)",
    "declare ptr @light_range(i64)",
    "declare ptr @light_str_from_utf8(ptr, i64)",
    "declare ptr @light_str_ptr(ptr)",
    "declare i64 @light_str_len(ptr)",
    "declare ptr @light_str_concat(ptr, ptr)",
    "declare void @light_str_free(ptr)",
    "declare i64 @light_str_index(ptr, i64)",
    "declare i64 @light_str_to_int(ptr)",
    "declare double @light_str_to_float(ptr)",
    "declare ptr @light_int_to_str(i64)",
    "declare ptr @light_float_to_str(double)",
    "declare ptr @light_bool_to_str(i64)",
    "declare ptr @light_input()",
    "declare i64 @light_pow_int(i64, i64)",
    "declare i64 @light_floor_div(i64, i64)",
    // 数学模块
    "declare double @light_math_sin(double)",
    "declare double @light_math_cos(double)",
    "declare double @light_math_tan(double)",
    "declare double @light_math_sqrt(double)",
    "declare double @light_math_log(double)",
    "declare double @light_math_log10(double)",
    "declare double @light_math_pow(double, double)",
    "declare double @light_math_pi()",
    "declare double @light_math_e()",
    "declare double @light_math_ceil(double)",
    "declare double @light_math_floor(double)",
    "declare double @light_math_random()",
    "declare i64 @light_math_random_int(i64, i64)",
    // 字符串模块
    "declare i64 @light_str_find(ptr, ptr)",
    "declare i1 @light_str_contains(ptr, ptr)",
    "declare ptr @light_str_replace(ptr, ptr, ptr)",
    "declare ptr @light_str_split(ptr, ptr)",
    "declare ptr @light_str_join(ptr, ptr)",
    "declare ptr @light_str_upper(ptr)",
    "declare ptr @light_str_lower(ptr)",
    "declare ptr @light_str_trim(ptr)",
    "declare ptr @light_str_substr(ptr, i64, i64)",
    // 数组扩展模块
    "declare i1 @light_array_contains(ptr, i64)",
    "declare i64 @light_array_index(ptr, i64)",
    "declare ptr @light_array_reverse(ptr)",
    "declare void @light_array_sort(ptr)",
    "declare ptr @light_array_slice(ptr, i64, i64)",
    "declare ptr @light_array_concat(ptr, ptr)",
    // 文件模块
    "declare ptr @light_file_read(ptr)",
    "declare i1 @light_file_write(ptr, ptr)",
    "declare i1 @light_file_append(ptr, ptr)",
    "declare i1 @light_file_exists(ptr)",
    // 系统模块
    "declare ptr @light_sys_exec(ptr)",
    "declare ptr @light_sys_getenv(ptr)",
    "declare void @light_sys_exit(i64)",
    // 工具
    "declare ptr @light_byte_to_char(i64)",
];

impl<'a> CodeGen<'a> {
    pub fn new(env: &'a typeinfer::TypeEnv, target_triple: String) -> Self {
        CodeGen {
            out: String::new(),
            tmp: 0,
            str_idx: 0,
            globals: Vec::new(),
            externs: HashSet::new(),
            env,
            loops: Vec::new(),
            deferred: Vec::new(),
            page_mode: false,
            target_triple,
        }
    }

    fn mangle(name: &str) -> String {
        let mut out = String::with_capacity(name.len() * 3);
        for c in name.chars() {
            if c.is_ascii_alphanumeric() || c == '_' {
                out.push(c);
            } else {
                out.push('_');
                for b in c.to_string().as_bytes() {
                    out.push_str(&format!("{:02x}", b));
                }
            }
        }
        out
    }

    fn fresh(&mut self) -> String {
        let n = self.tmp;
        self.tmp += 1;
        format!("%t{}", n)
    }

    fn emit(&mut self, s: &str) {
        self.out.push_str(s);
        if !s.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn fresh_label(&mut self, prefix: &str) -> String {
        let n = self.tmp;
        self.tmp += 1;
        format!("{}_{}", prefix, n)
    }

    fn i64_to_ptr(&mut self, v: &str) -> String {
        let r = self.fresh();
        self.emit(&format!("  {} = inttoptr i64 {} to ptr", r, v));
        r
    }

    fn ptr_to_i64(&mut self, v: &str) -> String {
        let r = self.fresh();
        self.emit(&format!("  {} = ptrtoint ptr {} to i64", r, v));
        r
    }

    /// 将表达式结果（可能是 double）存入统一的 i64 栈槽
    fn store_value(&mut self, slot: &str, val: &str, ty: Ty) {
        if ty.is_float() {
            let bc = self.fresh();
            self.emit(&format!("  {} = bitcast double {} to i64", bc, val));
            self.emit(&format!("  store i64 {}, ptr {}", bc, slot));
        } else {
            self.emit(&format!("  store i64 {}, ptr {}", val, slot));
        }
    }

    /// 从 i64 栈槽加载，按类型还原（浮点会转回 double）
    fn load_value(&mut self, slot: &str, ty: Ty) -> String {
        let r = self.fresh();
        self.emit(&format!("  {} = load i64, ptr {}", r, slot));
        if ty.is_float() {
            let f = self.fresh();
            self.emit(&format!("  {} = bitcast i64 {} to double", f, r));
            f
        } else {
            r
        }
    }

    /// 把一个值（int 或 float）提升为 double
    fn to_double(&mut self, val: &str, ty: Ty) -> String {
        if ty.is_float() {
            val.to_string()
        } else {
            let r = self.fresh();
            self.emit(&format!("  {} = sitofp i64 {} to double", r, val));
            r
        }
    }

    fn double_to_i64(&mut self, val: &str) -> String {
        let r = self.fresh();
        self.emit(&format!("  {} = fptosi double {} to i64", r, val));
        r
    }

    fn value_as_i64(&mut self, val: &str, ty: &Ty) -> String {
        if ty.is_float() {
            let r = self.fresh();
            self.emit(&format!("  {} = bitcast double {} to i64", r, val));
            r
        } else {
            val.to_string()
        }
    }

    fn value_tag(ty: &Ty) -> i32 {
        match ty {
            Ty::Int => 0,
            Ty::Float => 1,
            Ty::Str => 2,
            Ty::Bool => 3,
            Ty::None => 4,
            Ty::Array | Ty::List => 5,
            Ty::Dict => 6,
            Ty::Tuple => 7,
        }
    }

    fn free_value(&mut self, val: &str, ty: &Ty) {
        match ty {
            Ty::Str => {
                let p = self.i64_to_ptr(val);
                self.emit(&format!("  call void @light_str_free(ptr {})", p));
            }
            Ty::Array | Ty::List | Ty::Tuple => {
                let p = self.i64_to_ptr(val);
                self.emit(&format!("  call void @light_array_free(ptr {})", p));
            }
            Ty::Dict => {
                let p = self.i64_to_ptr(val);
                self.emit(&format!("  call void @light_dict_free(ptr {})", p));
            }
            _ => {}
        }
    }

    /// 将任意类型的值转为 i1（布尔真值）。整数/指针：非零为真；浮点：非 0.0 为真。
    fn gen_to_i1(&mut self, val: &str, ty: Ty) -> String {
        let r = self.fresh();
        if ty.is_float() {
            self.emit(&format!("  {} = fcmp one double {}, 0.0", r, val));
        } else {
            self.emit(&format!("  {} = icmp ne i64 {}, 0", r, val));
        }
        r
    }

    /// 将任意类型的值转为 i64 的 0/1 布尔值
    fn gen_to_i64_bool(&mut self, val: &str, ty: Ty) -> String {
        let i1 = self.gen_to_i1(val, ty);
        let r = self.fresh();
        self.emit(&format!("  {} = zext i1 {} to i64", r, i1));
        r
    }

    /// 创建一个全局字符串常量，返回 (gep 寄存器, 长度)
    fn emit_str_const(&mut self, s: &str) -> (String, usize) {
        let idx = self.str_idx;
        self.str_idx += 1;
        let bytes = s.as_bytes();
        let escaped = bytes.iter().map(|b| {
            match b {
                b'\n' => "\\0A".to_string(),
                b'\t' => "\\09".to_string(),
                b'\\' => "\\5C".to_string(),
                b'"' => "\\22".to_string(),
                0x00..=0x1f | 0x7f..=0xff => format!("\\{:02X}", b),
                _ => (*b as char).to_string(),
            }
        }).collect::<String>();
        let len = bytes.len();
        let global = format!(
            "@.str{} = private constant [{} x i8] c\"{}\"",
            idx, len, escaped
        );
        self.globals.push(global);
        let gep = self.fresh();
        self.emit(&format!(
            "  {} = getelementptr [{} x i8], ptr @.str{}, i64 0, i64 0",
            gep, len, idx
        ));
        (gep, len)
    }

fn free_var(&mut self, scopes: &[Scope], name: &str) {
    for scope in scopes.iter().rev() {
        if let Some((reg, ty)) = scope.vars.get(name) {
            let r = self.fresh();
            self.emit(&format!("  {} = load i64, ptr {}", r, reg));
            let p = self.i64_to_ptr(&r);
            match ty {
                Ty::Str => self.emit(&format!("  call void @light_str_free(ptr {})", p)),
                Ty::Array | Ty::List | Ty::Tuple => self.emit(&format!("  call void @light_array_free(ptr {})", p)),
                Ty::Dict => self.emit(&format!("  call void @light_dict_free(ptr {})", p)),
                _ => {}
            }
            return;
        }
    }
}

    fn lookup<'b>(&self, scopes: &'b [Scope], name: &str) -> Option<(&'b str, Ty)> {
        for scope in scopes.iter().rev() {
            if let Some((reg, ty)) = scope.vars.get(name) {
                return Some((reg.as_str(), *ty));
            }
        }
        None
    }

    fn mark_moved(scopes: &mut [Scope], name: &str) {
        for scope in scopes.iter_mut() {
            scope.owned.retain(|n| n != name);
        }
    }

fn cleanup_scope(&mut self, scope: &Scope) {
    for name in &scope.owned {
        if let Some((reg, ty)) = scope.vars.get(name) {
            let r = self.fresh();
            self.emit(&format!("  {} = load i64, ptr {}", r, reg));
            let p = self.i64_to_ptr(&r);
            match ty {
                Ty::Str => self.emit(&format!("  call void @light_str_free(ptr {})", p)),
                Ty::Array | Ty::List | Ty::Tuple => self.emit(&format!("  call void @light_array_free(ptr {})", p)),
                Ty::Dict => self.emit(&format!("  call void @light_dict_free(ptr {})", p)),
                _ => {}
            }
        }
    }
}

    pub fn gen(&mut self, program: &Program) -> Result<String, String> {
        self.emit("; Light 编译器生成的 LLVM IR");
        if !self.target_triple.is_empty() {
            self.emit(&format!("target triple = \"{}\"", self.target_triple));
        }
        self.emit("");
        for d in RUNTIME_DECLS {
            self.emit(d);
        }
        self.emit("");

        let mut externs = HashMap::new();
        for item in &program.items {
            if let Item::Extern(e) = item {
                externs.insert(e.name.clone(), e.params.len());
            }
        }

        for (name, nargs) in &externs {
            self.externs.insert(name.clone());
            let params = (0..*nargs).map(|_| "i64").collect::<Vec<_>>().join(", ");
            self.emit(&format!("declare i64 @{}({})", name, params));
        }
        self.emit("");

        for item in &program.items {
            match item {
                Item::Fn(f) => self.gen_fn(f)?,
                Item::Extern(_) => {}
                Item::Import(_) => {}  // 导入已在解析阶段处理
                Item::Stmt(_) => {}
            }
        }

        // 收集顶层语句，自动包装为 main()
        let top_stmts: Vec<&Stmt> = program.items.iter()
            .filter_map(|item| if let Item::Stmt(s) = item { Some(s) } else { None })
            .collect();
        let has_page = top_stmts.iter().any(|stmt| matches!(stmt, Stmt::Page { .. }));
        let has_main = program.items.iter().any(|item| {
            matches!(item, Item::Fn(f) if f.name == "main")
        });
        if !has_main {
            self.emit("define i64 @main() {");
            self.emit("entry:");
            let scope = Scope { vars: HashMap::new(), owned: Vec::new() };
            let mut scopes = vec![scope];
            for stmt in &top_stmts {
                self.gen_stmt(stmt, &mut scopes)?;
            }
            if has_page {
                self.emit("  call i64 @light_web_run_v1(i64 0)");
            }
            self.emit("  call i64 @light_thread_join_all()");
            let top = scopes.first().unwrap();
            self.cleanup_scope(top);
            self.emit("  ret i64 0");
            self.emit("}");
            self.emit("");
        }

        let mut result = String::new();
        result.push_str(&self.out);
        for function in &self.deferred {
            result.push_str(function);
            if !function.ends_with('\n') {
                result.push('\n');
            }
        }
        for g in &self.globals {
            result.push_str(g);
            result.push('\n');
        }
        Ok(result)
    }

    fn gen_fn(&mut self, f: &FnDef) -> Result<(), String> {
        let params: Vec<String> = (0..f.params.len())
            .map(|i| format!("%p{}", i))
            .collect();
        let param_list = if params.is_empty() {
            String::new()
        } else {
            params.iter().map(|p| format!("i64 {}", p)).collect::<Vec<_>>().join(", ")
        };
        self.emit(&format!(
            "define i64 @{}({}) {{",
            Self::mangle(&f.name), param_list
        ));
        self.emit("entry:");

        let mut scope = Scope {
            vars: HashMap::new(),
            owned: Vec::new(),
        };
        for (i, name) in f.params.iter().enumerate() {
            let reg = self.fresh();
            self.emit(&format!("  {} = alloca i64", reg));
            self.emit(&format!("  store i64 %p{}, ptr {}", i, reg));
            let pty = self.env.param_types.get(&(f.name.clone(), i)).cloned();
            let ty: Ty = pty.map(|t| t.into()).unwrap_or(Ty::Int);
            scope.vars.insert(name.clone(), (reg, ty));
            // 参数按 move 语义拥有：若参数是字符串/数组，由本函数负责释放
            if ty.is_owned() {
                scope.owned.push(name.clone());
            }
        }

        let mut scopes = vec![scope];
        self.gen_block(&f.body, &mut scopes)?;

        // 若函数体最后一条语句是返回，则不再追加默认 ret（避免死代码与重复释放）
        let ends_with_return = f.body.stmts.last()
            .map(|s| matches!(s, Stmt::Return(_)))
            .unwrap_or(false);
        if !ends_with_return {
            let top = scopes.first().unwrap();
            self.cleanup_scope(top);
            self.emit("  ret i64 0");
        }
        self.emit("}");
        self.emit("");
        Ok(())
    }

    fn gen_block(&mut self, block: &Block, scopes: &mut Vec<Scope>) -> Result<(), String> {
        let scope = Scope { vars: HashMap::new(), owned: Vec::new() };
        scopes.push(scope);
        for stmt in &block.stmts {
            self.gen_stmt(stmt, scopes)?;
        }
        let scope = scopes.pop().unwrap();
        // 若块以返回语句结尾，返回已完成清理，不再重复（避免 ret 后出现死代码）
        let ends_with_return = block.stmts.last()
            .map(|s| matches!(s, Stmt::Return(_)))
            .unwrap_or(false);
        if !ends_with_return {
            self.cleanup_scope(&scope);
        }
        Ok(())
    }

    fn gen_throw_call(&mut self, expr: &Expr, scopes: &mut Vec<Scope>) -> Result<(), String> {
        let (reg, ty) = self.gen_expr(expr, scopes)?;
        if ty != Ty::Str {
            return Err("抛出内容必须是字符串".into());
        }
        let p = self.i64_to_ptr(&reg);
        self.emit(&format!("  call void @light_throw(ptr {})", p));
        if !matches!(expr, Expr::Ident(_)) {
            self.emit(&format!("  call void @light_str_free(ptr {})", p));
        }
        Ok(())
    }

    fn gen_try(&mut self, body: &Block, catch: Option<&(String, Block)>,
               finally: Option<&Block>, scopes: &mut Vec<Scope>) -> Result<(), String> {
        let catch_label = self.fresh_label("trycatch");
        let finally_label = self.fresh_label("tryfinally");
        let end_label = self.fresh_label("tryend");
        let body_scope = Scope { vars: HashMap::new(), owned: Vec::new() };
        scopes.push(body_scope);
        let mut terminated = false;
        for stmt in &body.stmts {
            if let Stmt::Throw(expr) = stmt {
                self.gen_throw_call(expr, scopes)?;
                let body_scope = scopes.pop().unwrap();
                self.cleanup_scope(&body_scope);
                let target = if catch.is_some() { &catch_label } else { &finally_label };
                self.emit(&format!("  br label %{}", target));
                terminated = true;
                break;
            }
            self.gen_stmt(stmt, scopes)?;
        }
        if !terminated {
            let body_scope = scopes.pop().unwrap();
            self.cleanup_scope(&body_scope);
            self.emit(&format!("  br label %{}", finally_label));
        }

        if let Some((name, block)) = catch {
            self.emit(&format!("{}:", catch_label));
            let error_ptr = self.fresh();
            self.emit(&format!("  {} = call ptr @light_take_error()", error_ptr));
            let error_reg = self.ptr_to_i64(&error_ptr);
            let slot = self.fresh();
            self.emit(&format!("  {} = alloca i64", slot));
            self.emit(&format!("  store i64 {}, ptr {}", error_reg, slot));
            let catch_scope = Scope { vars: HashMap::new(), owned: Vec::new() };
            scopes.push(catch_scope);
            scopes.last_mut().unwrap().vars.insert(name.clone(), (slot, Ty::Str));
            scopes.last_mut().unwrap().owned.push(name.clone());
            for stmt in &block.stmts {
                self.gen_stmt(stmt, scopes)?;
            }
            let catch_scope = scopes.pop().unwrap();
            self.cleanup_scope(&catch_scope);
            self.emit(&format!("  br label %{}", finally_label));
        }

        self.emit(&format!("{}:", finally_label));
        if let Some(block) = finally {
            self.gen_block(block, scopes)?;
        }
        self.emit(&format!("  br label %{}", end_label));
        self.emit(&format!("{}:", end_label));
        Ok(())
    }

    fn gen_page_print(&mut self, expr: &Expr, scopes: &mut Vec<Scope>) -> Result<(), String> {
        let (reg, ty) = self.gen_expr(expr, scopes)?;
        match ty {
            Ty::Str => {
                let p = self.i64_to_ptr(&reg);
                let data = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_ptr(ptr {})", data, p));
                let len = self.fresh();
                self.emit(&format!("  {} = call i64 @light_str_len(ptr {})", len, p));
                self.emit(&format!("  call void @light_page_append_str(ptr {}, i64 {})", data, len));
                if !matches!(expr, Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", p));
                }
            }
            Ty::Int => self.emit(&format!("  call void @light_page_append_int(i64 {})", reg)),
            Ty::Float => self.emit(&format!("  call void @light_page_append_float(double {})", reg)),
            Ty::Bool => self.emit(&format!("  call void @light_page_append_bool(i64 {})", reg)),
            _ => return Err("页面中只能打印字符串、数字或布尔值".into()),
        }
        Ok(())
    }

    fn gen_page_setting(&mut self, name: &str, value: &Expr, scopes: &mut Vec<Scope>) -> Result<bool, String> {
        if !self.page_mode || !matches!(name, "语言" | "主题") {
            return Ok(false);
        }
        let (reg, data, len) = self.gen_string_parts(value, scopes)?;
        let function = if name == "语言" {
            "light_page_set_lang"
        } else {
            "light_page_set_theme"
        };
        self.emit(&format!("  call void @{}(ptr {}, i64 {})", function, data, len));
        if !matches!(value, Expr::Ident(_)) {
            self.free_value(&reg, &Ty::Str);
        }
        Ok(true)
    }

    fn gen_page(&mut self, title: &str, icon: &str, body: &Block) -> Result<(), String> {
        let symbol = self.fresh_label("__light_page");
        let saved = std::mem::take(&mut self.out);
        self.emit(&format!("define i64 @{}(i64 %request) {{", symbol));
        self.emit("entry:");
        let (title_ptr, title_len) = self.emit_str_const(title);
        let (icon_ptr, icon_len) = self.emit_str_const(icon);
        self.emit(&format!(
            "  call void @light_page_reset(ptr {}, i64 {}, ptr {}, i64 {})",
            title_ptr, title_len, icon_ptr, icon_len
        ));
        let previous_mode = std::mem::replace(&mut self.page_mode, true);
        self.gen_block(body, &mut vec![Scope { vars: HashMap::new(), owned: Vec::new() }])?;
        self.page_mode = previous_mode;
        let ends_with_return = body.stmts.last()
            .map(|stmt| matches!(stmt, Stmt::Return(_)))
            .unwrap_or(false);
        if !ends_with_return {
            let page = self.fresh();
            self.emit(&format!("  {} = call ptr @light_page_finish()", page));
            let result = self.fresh();
            self.emit(&format!("  {} = ptrtoint ptr {} to i64", result, page));
            self.emit(&format!("  ret i64 {}", result));
        }
        self.emit("}");
        self.emit("");
        let wrapper = self.out.clone();
        self.out = saved;
        self.deferred.push(wrapper);
        let (path_ptr, path_len) = self.emit_str_const("/");
        let status = self.fresh();
        self.emit(&format!(
            "  {} = call i64 @light_web_register_html_v1(i32 0, ptr {}, i64 {}, ptr @{})",
            status, path_ptr, path_len, symbol
        ));
        Ok(())
    }

    fn gen_thread(&mut self, id: i64, body: &Block) -> Result<(), String> {
        let symbol = self.fresh_label("__light_thread");
        let saved = std::mem::take(&mut self.out);
        self.emit(&format!("define i64 @{}(i64 %thread_id) {{", symbol));
        self.emit("entry:");
        let scope = Scope { vars: HashMap::new(), owned: Vec::new() };
        let mut scopes = vec![scope];
        self.gen_block(body, &mut scopes)?;
        let ends_with_return = body.stmts.last()
            .map(|stmt| matches!(stmt, Stmt::Return(_)))
            .unwrap_or(false);
        if !ends_with_return {
            self.emit("  ret i64 0");
        }
        self.emit("}");
        self.emit("");
        let wrapper = self.out.clone();
        self.out = saved;
        self.deferred.push(wrapper);
        let task = self.fresh();
        self.emit(&format!(
            "  {} = call ptr @light_thread_spawn(ptr @{}, i64 {})",
            task, symbol, id
        ));
        Ok(())
    }

    fn gen_stmt(&mut self, stmt: &Stmt, scopes: &mut Vec<Scope>) -> Result<(), String> {
        match stmt {
            Stmt::Let { name, value } => {
                if self.gen_page_setting(name, value, scopes)? {
                    return Ok(());
                }
                let (val_reg, ty) = self.gen_expr(value, scopes)?;
                let reg = self.fresh();
                self.emit(&format!("  {} = alloca i64", reg));
                self.store_value(&reg, &val_reg, ty);
                let scope = scopes.last_mut().unwrap();
                scope.vars.insert(name.clone(), (reg.clone(), ty));
                if ty.is_owned() {
                    scope.owned.push(name.clone());
                }
            }
            Stmt::Assign { target, value } => {
                match target {
                    Expr::Ident(name) => {
                        if self.gen_page_setting(name, value, scopes)? {
                            return Ok(());
                        }
                        let old_ty = self.lookup(scopes, name).map(|(_, ty)| ty);
                        let (val_reg, new_ty) = self.gen_expr(value, scopes)?;
                        if let Some(old_ty) = old_ty {
                            if old_ty.is_owned() {
                                self.free_var(scopes, name);
                                Self::mark_moved(scopes, name);
                            }
                            let (reg, _) = self.lookup(scopes, name).unwrap();
                            self.store_value(reg, &val_reg, new_ty);
                            let scope = scopes.last_mut().unwrap();
                            if let Some((_, ref mut t)) = scope.vars.get_mut(name) {
                                *t = new_ty;
                            }
                            if new_ty.is_owned() && !scope.owned.contains(name) {
                                scope.owned.push(name.clone());
                            }
                        } else {
                            let reg = self.fresh();
                            self.emit(&format!("  {} = alloca i64", reg));
                            self.store_value(&reg, &val_reg, new_ty);
                            let scope = scopes.last_mut().unwrap();
                            scope.vars.insert(name.clone(), (reg, new_ty));
                            if new_ty.is_owned() {
                                scope.owned.push(name.clone());
                            }
                        }
                    }
                    Expr::Index { arr, idx } => {
                        let (arr_reg, arr_ty) = self.gen_expr(arr, scopes)?;
                        if !matches!(arr_ty, Ty::Array | Ty::List) {
                            return Err("只能对数组或列表使用索引赋值".into());
                        }
                        let (idx_reg, _) = self.gen_expr(idx, scopes)?;
                        let (val_reg, vty) = self.gen_expr(value, scopes)?;
                        let val_i64 = self.value_as_i64(&val_reg, &vty);
                        let ap = self.i64_to_ptr(&arr_reg);
                        self.emit(&format!(
                            "  call void @light_array_set_copy_tag(ptr {}, i64 {}, i32 {}, i64 {})",
                            ap,
                            idx_reg,
                            Self::value_tag(&vty),
                            val_i64
                        ));
                        if !matches!(arr.as_ref(), Expr::Ident(_)) {
                            self.free_value(&arr_reg, &arr_ty);
                        }
                        if !matches!(value, Expr::Ident(_)) {
                            self.free_value(&val_reg, &vty);
                        }
                    }
                    _ => return Err("赋值目标必须是变量或数组元素".into()),
                }
            }
            Stmt::Expr(e) => {
                let _ = self.gen_expr(e, scopes)?;
            }
            Stmt::Print(e) if self.page_mode => {
                self.gen_page_print(e, scopes)?;
            }
            Stmt::Print(e) => {
                let (reg, ty) = self.gen_expr(e, scopes)?;
                match ty {
                    Ty::Int => {
                        self.emit(&format!("  call void @light_print_int(i64 {})", reg));
                        self.emit("  call void @light_print_newline()");
                    }
                    Ty::Float => {
                        self.emit(&format!("  call void @light_print_float(double {})", reg));
                        self.emit("  call void @light_print_newline()");
                    }
                    Ty::Bool => {
                        self.emit(&format!("  call void @light_print_bool(i64 {})", reg));
                        self.emit("  call void @light_print_newline()");
                    }
                    Ty::Str => {
                        let p = self.i64_to_ptr(&reg);
                        let ptr_r = self.fresh();
                        self.emit(&format!("  {} = call ptr @light_str_ptr(ptr {})", ptr_r, p));
                        let len_r = self.fresh();
                        self.emit(&format!("  {} = call i64 @light_str_len(ptr {})", len_r, p));
                        self.emit(&format!(
                            "  call void @light_print_str(ptr {}, i64 {})",
                            ptr_r, len_r
                        ));
                        self.emit("  call void @light_print_newline()");
                        if !matches!(e, Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_str_free(ptr {})", p));
                        }
                    }
                    Ty::Array | Ty::List => {
                        let p = self.i64_to_ptr(&reg);
                        self.emit(&format!("  call void @light_print_array(ptr {})", p));
                        self.emit("  call void @light_print_newline()");
                        if !matches!(e, Expr::Ident(_)) {
                            self.free_value(&reg, &ty);
                        }
                    }
                    Ty::Tuple => {
                        let p = self.i64_to_ptr(&reg);
                        self.emit(&format!("  call void @light_print_tuple(ptr {})", p));
                        self.emit("  call void @light_print_newline()");
                        if !matches!(e, Expr::Ident(_)) {
                            self.free_value(&reg, &ty);
                        }
                    }
                    Ty::Dict => {
                        let p = self.i64_to_ptr(&reg);
                        self.emit(&format!("  call void @light_print_dict(ptr {})", p));
                        self.emit("  call void @light_print_newline()");
                        if !matches!(e, Expr::Ident(_)) {
                            self.free_value(&reg, &ty);
                        }
                    }
                    Ty::None => {
                        // 打印字面量 "空"
                        let (gep, len) = self.emit_str_const("空");
                        self.emit(&format!("  call void @light_print_str(ptr {}, i64 {})", gep, len));
                        self.emit("  call void @light_print_newline()");
                    }
                }
            }
            Stmt::Thread { id, body } => {
                self.gen_thread(*id, body)?;
            }
            Stmt::Page { title, icon, body } => {
                self.gen_page(title, icon, body)?;
            }
            Stmt::Throw(expr) => {
                self.gen_throw_call(expr, scopes)?;
                self.emit("  call void @light_sys_exit(i64 1)");
            }
            Stmt::Try { body, catch, finally } => {
                self.gen_try(body, catch.as_ref(), finally.as_ref(), scopes)?;
            }
            Stmt::If { cond, then, elifs, els } => {
                self.gen_if_chain(cond, then, elifs, els, scopes)?;
            }
            Stmt::While { cond, body } => {
                let head_lbl = self.fresh_label("whilehead");
                let body_lbl = self.fresh_label("whilebody");
                let end_lbl = self.fresh_label("whileend");
                self.emit(&format!("  br label %{}", head_lbl));
                self.emit(&format!("{}:", head_lbl));
                let (cond_reg, cond_ty) = self.gen_expr(cond, scopes)?;
                let c1 = self.gen_to_i1(&cond_reg, cond_ty);
                self.emit(&format!(
                    "  br i1 {}, label %{}, label %{}",
                    c1, body_lbl, end_lbl
                ));
                self.emit(&format!("{}:", body_lbl));
                self.loops.push(LoopCtx { brk: end_lbl.clone(), cont: head_lbl.clone() });
                self.gen_block(body, scopes)?;
                self.loops.pop();
                self.emit(&format!("  br label %{}", head_lbl));
                self.emit(&format!("{}:", end_lbl));
            }
            Stmt::For { var, iter, body } => {
                // 对于 var 在 迭代:  => 把迭代当作数组，按下标遍历
                let (iter_reg, iter_ty) = self.gen_expr(iter, scopes)?;
                if iter_ty != Ty::Array {
                    return Err("for-in 只能遍历数组（用 范围(n) 生成序列）".into());
                }
                let ap = self.i64_to_ptr(&iter_reg);
                let len_r = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_len(ptr {})", len_r, ap));
                // 循环计数器
                let i_reg = self.fresh();
                self.emit(&format!("  {} = alloca i64", i_reg));
                self.emit(&format!("  store i64 0, ptr {}", i_reg));
                let head_lbl = self.fresh_label("forhead");
                let body_lbl = self.fresh_label("forbody");
                let step_lbl = self.fresh_label("forstep");
                let end_lbl = self.fresh_label("forend");
                self.emit(&format!("  br label %{}", head_lbl));
                self.emit(&format!("{}:", head_lbl));
                let iv = self.fresh();
                self.emit(&format!("  {} = load i64, ptr {}", iv, i_reg));
                let cmp = self.fresh();
                self.emit(&format!("  {} = icmp slt i64 {}, {}", cmp, iv, len_r));
                self.emit(&format!("  br i1 {}, label %{}, label %{}", cmp, body_lbl, end_lbl));
                self.emit(&format!("{}:", body_lbl));
                // var = arr[i]
                let elem = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_get(ptr {}, i64 {})", elem, ap, iv));
                let vreg = self.fresh();
                self.emit(&format!("  {} = alloca i64", vreg));
                self.emit(&format!("  store i64 {}, ptr {}", elem, vreg));
                let scope = scopes.last_mut().unwrap();
                scope.vars.insert(var.clone(), (vreg.clone(), Ty::Int));
                // break → end, continue → step（先自增再判断）
                self.loops.push(LoopCtx { brk: end_lbl.clone(), cont: step_lbl.clone() });
                self.gen_block(body, scopes)?;
                self.loops.pop();
                self.emit(&format!("  br label %{}", step_lbl));
                self.emit(&format!("{}:", step_lbl));
                // i += 1
                let iv2 = self.fresh();
                self.emit(&format!("  {} = load i64, ptr {}", iv2, i_reg));
                let iv3 = self.fresh();
                self.emit(&format!("  {} = add i64 {}, 1", iv3, iv2));
                self.emit(&format!("  store i64 {}, ptr {}", iv3, i_reg));
                self.emit(&format!("  br label %{}", head_lbl));
                self.emit(&format!("{}:", end_lbl));
                // 遍历完后若 iter 是临时（非变量），释放数组
                if !matches!(iter, Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
            }
            Stmt::Break => {
                let ctx = self.loops.last()
                    .ok_or_else(|| "跳出 只能在循环内使用".to_string())?;
                self.emit(&format!("  br label %{}", ctx.brk));
            }
            Stmt::Continue => {
                let ctx = self.loops.last()
                    .ok_or_else(|| "继续 只能在循环内使用".to_string())?;
                self.emit(&format!("  br label %{}", ctx.cont));
            }
            Stmt::Return(e) => {
                let val = match e {
                    Some(expr) => {
                        let (r, ty) = self.gen_expr(expr, scopes)?;
                        if ty.is_owned() {
                            if let Expr::Ident(n) = expr {
                                Self::mark_moved(scopes, n);
                            }
                        }
                        // 浮点返回：bitcast 为 i64
                        if ty.is_float() {
                            let bc = self.fresh();
                            self.emit(&format!("  {} = bitcast double {} to i64", bc, r));
                            bc
                        } else {
                            r
                        }
                    }
                    None => "0".to_string(),
                };
                for scope in scopes.iter().rev() {
                    self.cleanup_scope(scope);
                }
                self.emit(&format!("  ret i64 {}", val));
            }
            Stmt::Asm(raw) => {
                self.emit(&format!("  call void asm sideeffect \"{}\", \"~{{memory}}\"()",
                    raw.replace('"', "\\\"").replace('\n', "\\n ")));
            }
        }
        Ok(())
    }

    /// 生成 if / elif* / else? 链
    fn gen_if_chain(&mut self, cond: &Expr, then: &Block,
                     elifs: &[(Expr, Block)], els: &Option<Block>,
                     scopes: &mut Vec<Scope>) -> Result<(), String> {
        let end_lbl = self.fresh_label("ifend");
        let mut next_cond_lbl = self.fresh_label("elifcond");

        // 第一个 if
        let (cond_reg, cond_ty) = self.gen_expr(cond, scopes)?;
        let c1 = self.gen_to_i1(&cond_reg, cond_ty);
        let then_lbl = self.fresh_label("then");
        let fallthrough = if !elifs.is_empty() { next_cond_lbl.clone() }
                          else if els.is_some() { self.fresh_label("else") }
                          else { end_lbl.clone() };
        self.emit(&format!("  br i1 {}, label %{}, label %{}", c1, then_lbl, fallthrough));
        self.emit(&format!("{}:", then_lbl));
        self.gen_block(then, scopes)?;
        self.emit(&format!("  br label %{}", end_lbl));

        // elif 链
        let mut prev_fallthrough = fallthrough.clone();
        for (i, (ec, eb)) in elifs.iter().enumerate() {
            self.emit(&format!("{}:", prev_fallthrough));
            let (er, ety) = self.gen_expr(ec, scopes)?;
            let c = self.gen_to_i1(&er, ety);
            let ethen = self.fresh_label("elifbody");
            let next = if i + 1 < elifs.len() {
                next_cond_lbl = self.fresh_label("elifcond");
                next_cond_lbl.clone()
            } else if els.is_some() {
                self.fresh_label("else")
            } else {
                end_lbl.clone()
            };
            self.emit(&format!("  br i1 {}, label %{}, label %{}", c, ethen, next));
            self.emit(&format!("{}:", ethen));
            self.gen_block(eb, scopes)?;
            self.emit(&format!("  br label %{}", end_lbl));
            prev_fallthrough = next;
        }

        // else
        if let Some(eb) = els {
            self.emit(&format!("{}:", prev_fallthrough));
            self.gen_block(eb, scopes)?;
            self.emit(&format!("  br label %{}", end_lbl));
        }

        self.emit(&format!("{}:", end_lbl));
        Ok(())
    }

    fn gen_sequence(&mut self, items: &[Expr], scopes: &mut Vec<Scope>) -> Result<(String, Vec<Ty>), String> {
        let mut item_regs = Vec::new();
        let mut item_tys = Vec::new();
        for item in items {
            let (reg, ty) = self.gen_expr(item, scopes)?;
            item_regs.push(reg);
            item_tys.push(ty);
        }
        let len = items.len();
        let arr = self.fresh();
        self.emit(&format!("  {} = call ptr @light_array_new(i64 {})", arr, len));
        for (i, (item, reg)) in items.iter().zip(item_regs.iter()).enumerate() {
            let value = self.value_as_i64(reg, &item_tys[i]);
            self.emit(&format!(
                "  call void @light_array_set_copy_tag(ptr {}, i64 {}, i32 {}, i64 {})",
                arr,
                i,
                Self::value_tag(&item_tys[i]),
                value
            ));
            if !matches!(item, Expr::Ident(_)) {
                self.free_value(reg, &item_tys[i]);
            }
        }
        let r = self.ptr_to_i64(&arr);
        Ok((r, item_tys))
    }

    fn gen_expr(&mut self, expr: &Expr, scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        match expr {
            Expr::Int(n) => {
                let r = self.fresh();
                self.emit(&format!("  {} = add i64 0, {}", r, n));
                Ok((r, Ty::Int))
            }
            Expr::Float(n) => {
                let r = self.fresh();
                // LLVM 浮点常量用科学计数法表示
                let s = format_float_const(*n);
                self.emit(&format!("  {} = fadd double 0.0, {}", r, s));
                Ok((r, Ty::Float))
            }
            Expr::Bool(b) => {
                let r = self.fresh();
                self.emit(&format!("  {} = add i64 0, {}", r, if *b { 1 } else { 0 }));
                Ok((r, Ty::Bool))
            }
            Expr::None => {
                let r = self.fresh();
                self.emit(&format!("  {} = add i64 0, 0", r));
                Ok((r, Ty::None))
            }
            Expr::Tuple(items) => {
                let (r, _) = self.gen_sequence(items, scopes)?;
                Ok((r, Ty::Tuple))
            }
            Expr::List(items) => {
                let (r, _) = self.gen_sequence(items, scopes)?;
                Ok((r, Ty::List))
            }
            Expr::Dict(entries) => {
                let dict = self.fresh();
                self.emit(&format!("  {} = call ptr @light_dict_new()", dict));
                for (key, value) in entries {
                    let (key_reg, key_ty) = self.gen_expr(key, scopes)?;
                    let (value_reg, value_ty) = self.gen_expr(value, scopes)?;
                    let key_value = self.value_as_i64(&key_reg, &key_ty);
                    let value_value = self.value_as_i64(&value_reg, &value_ty);
                    self.emit(&format!(
                        "  call void @light_dict_set(ptr {}, i32 {}, i64 {}, i32 {}, i64 {})",
                        dict,
                        Self::value_tag(&key_ty),
                        key_value,
                        Self::value_tag(&value_ty),
                        value_value
                    ));
                    if !matches!(key, Expr::Ident(_)) {
                        self.free_value(&key_reg, &key_ty);
                    }
                    if !matches!(value, Expr::Ident(_)) {
                        self.free_value(&value_reg, &value_ty);
                    }
                }
                let r = self.ptr_to_i64(&dict);
                Ok((r, Ty::Dict))
            }
            Expr::Str(s) => {
                let idx = self.str_idx;
                self.str_idx += 1;
                let bytes = s.as_bytes();
                let escaped = bytes.iter().map(|b| {
                    match b {
                        b'\n' => "\\0A".to_string(),
                        b'\t' => "\\09".to_string(),
                        b'\\' => "\\5C".to_string(),
                        b'"' => "\\22".to_string(),
                        0x00..=0x1f | 0x7f..=0xff => format!("\\{:02X}", b),
                        _ => (*b as char).to_string(),
                    }
                }).collect::<String>();
                let len = bytes.len();
                let global = format!(
                    "@.str{} = private constant [{} x i8] c\"{}\"",
                    idx, len, escaped
                );
                self.globals.push(global);
                let gep = self.fresh();
                self.emit(&format!(
                    "  {} = getelementptr [{} x i8], ptr @.str{}, i64 0, i64 0",
                    gep, len, idx
                ));
                let sp = self.fresh();
                self.emit(&format!(
                    "  {} = call ptr @light_str_from_utf8(ptr {}, i64 {})",
                    sp, gep, len
                ));
                let r = self.ptr_to_i64(&sp);
                Ok((r, Ty::Str))
            }
            Expr::Ident(name) => {
                let (reg, ty) = self.lookup(scopes, name)
                    .ok_or_else(|| format!("未定义变量：{}", name))?;
                let r = self.load_value(reg, ty);
                Ok((r, ty))
            }
            Expr::UnaryOp { op, operand } => {
                let (or_, oty) = self.gen_expr(operand, scopes)?;
                match op {
                    UnaryOp::Neg => {
                        if oty.is_float() {
                            let r = self.fresh();
                            self.emit(&format!("  {} = fsub double -0.0, {}", r, or_));
                            Ok((r, Ty::Float))
                        } else {
                            let r = self.fresh();
                            self.emit(&format!("  {} = sub i64 0, {}", r, or_));
                            Ok((r, Ty::Int))
                        }
                    }
                    UnaryOp::Not => {
                        let r = self.gen_to_i64_bool(&or_, oty);
                        // 取反：0→1, 1→0
                        let r2 = self.fresh();
                        self.emit(&format!("  {} = xor i64 {}, 1", r2, r));
                        Ok((r2, Ty::Bool))
                    }
                }
            }
            Expr::BinOp { op, lhs, rhs } => {
                use BinOp::*;
                match op {
                    And | Or => {
                        // 非短路：两侧都求值，然后做逻辑与/或
                        let (lr, lt) = self.gen_expr(lhs, scopes)?;
                        let (rr, rt) = self.gen_expr(rhs, scopes)?;
                        let lb = self.gen_to_i1(&lr, lt);
                        let rb = self.gen_to_i1(&rr, rt);
                        let r1 = self.fresh();
                        if matches!(op, And) {
                            self.emit(&format!("  {} = and i1 {}, {}", r1, lb, rb));
                        } else {
                            self.emit(&format!("  {} = or i1 {}, {}", r1, lb, rb));
                        }
                        let r2 = self.fresh();
                        self.emit(&format!("  {} = zext i1 {} to i64", r2, r1));
                        Ok((r2, Ty::Bool))
                    }
                    Add | Sub | Mul | Div | FloorDiv | Mod | Pow => {
                        self.gen_arith(*op, lhs, rhs, scopes)
                    }
                    Eq | Neq | Lt | Gt | Le | Ge => {
                        self.gen_cmp(*op, lhs, rhs, scopes)
                    }
                }
            }
            Expr::Call { name, args } => {
                self.gen_call(name, args, scopes)
            }
            Expr::Index { arr, idx } => {
                let (arr_reg, arr_ty) = self.gen_expr(arr, scopes)?;
                let (idx_reg, _) = self.gen_expr(idx, scopes)?;
                match arr_ty {
                    Ty::Array | Ty::List => {
                        let ap = self.i64_to_ptr(&arr_reg);
                        let r = self.fresh();
                        self.emit(&format!(
                            "  {} = call i64 @light_array_get(ptr {}, i64 {})",
                            r, ap, idx_reg
                        ));
                        if !matches!(arr.as_ref(), Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                        }
                        Ok((r, Ty::Int))
                    }
                    Ty::Str => {
                        let sp = self.i64_to_ptr(&arr_reg);
                        let r = self.fresh();
                        self.emit(&format!(
                            "  {} = call i64 @light_str_index(ptr {}, i64 {})",
                            r, sp, idx_reg
                        ));
                        if !matches!(arr.as_ref(), Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                        }
                        Ok((r, Ty::Int))
                    }
                    _ => Err("只能对数组或字符串使用索引".into()),
                }
            }
            Expr::ArrayNew(len_expr) => {
                let (len_reg, _) = self.gen_expr(len_expr, scopes)?;
                let ap = self.fresh();
                self.emit(&format!("  {} = call ptr @light_array_new(i64 {})", ap, len_reg));
                let r = self.ptr_to_i64(&ap);
                Ok((r, Ty::Array))
            }
        }
    }

    /// 算术运算（含浮点提升）
    fn gen_arith(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr,
                  scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        let (lr, lt) = self.gen_expr(lhs, scopes)?;
        let (rr, rt) = self.gen_expr(rhs, scopes)?;

        // 字符串拼接
        if lt == Ty::Str || rt == Ty::Str {
            if lt != Ty::Str || rt != Ty::Str {
                return Err("字符串只能与字符串拼接".into());
            }
            let lp = self.i64_to_ptr(&lr);
            let rp = self.i64_to_ptr(&rr);
            let cp = self.fresh();
            self.emit(&format!("  {} = call ptr @light_str_concat(ptr {}, ptr {})", cp, lp, rp));
            if !matches!(lhs, Expr::Ident(_)) {
                self.emit(&format!("  call void @light_str_free(ptr {})", lp));
            }
            if !matches!(rhs, Expr::Ident(_)) {
                self.emit(&format!("  call void @light_str_free(ptr {})", rp));
            }
            let r = self.ptr_to_i64(&cp);
            return Ok((r, Ty::Str));
        }

        let is_float = lt.is_float() || rt.is_float();
        // 除法（非整除）总是浮点
        let force_float = matches!(op, BinOp::Div);

        if is_float || force_float {
            let lf = self.to_double(&lr, lt);
            let rf = self.to_double(&rr, rt);
            let r = self.fresh();
            let inst = match op {
                BinOp::Add => "fadd",
                BinOp::Sub => "fsub",
                BinOp::Mul => "fmul",
                BinOp::Div => "fdiv",
                BinOp::FloorDiv => {
                    // 浮点整除：fdiv 后取整
                    let q = self.fresh();
                    self.emit(&format!("  {} = fdiv double {}, {}", q, lf, rf));
                    let floor = self.fresh();
                    self.emit(&format!("  {} = call double @llvm.floor.f64(double {})", floor, q));
                    return Ok((floor, Ty::Float));
                }
                BinOp::Mod => {
                    let q = self.fresh();
                    self.emit(&format!("  {} = frem double {}, {}", q, lf, rf));
                    return Ok((q, Ty::Float));
                }
                BinOp::Pow => {
                    let q = self.fresh();
                    self.emit(&format!("  {} = call double @llvm.pow.f64(double {}, double {})", q, lf, rf));
                    return Ok((q, Ty::Float));
                }
                _ => unreachable!(),
            };
            self.emit(&format!("  {} = {} double {}, {}", r, inst, lf, rf));
            Ok((r, Ty::Float))
        } else {
            // 整数运算
            match op {
                BinOp::FloorDiv => {
                    let r = self.fresh();
                    self.emit(&format!("  {} = call i64 @light_floor_div(i64 {}, i64 {})", r, lr, rr));
                    Ok((r, Ty::Int))
                }
                BinOp::Pow => {
                    let r = self.fresh();
                    self.emit(&format!("  {} = call i64 @light_pow_int(i64 {}, i64 {})", r, lr, rr));
                    Ok((r, Ty::Int))
                }
                BinOp::Mod => {
                    let r = self.fresh();
                    self.emit(&format!("  {} = srem i64 {}, {}", r, lr, rr));
                    Ok((r, Ty::Int))
                }
                BinOp::Div => {
                    // 整数除法（当两边都是 int 时，Div 走这里；但类型推断说 Div 返回 Float）
                    // 这里保守地仍做整数 sdiv。若需要浮点，上面 is_float 分支已处理。
                    // 实际上类型推断 Div=>Float，所以这里不会被命中（force_float 已处理）。
                    let r = self.fresh();
                    self.emit(&format!("  {} = sdiv i64 {}, {}", r, lr, rr));
                    Ok((r, Ty::Int))
                }
                _ => {
                    let r = self.fresh();
                    let inst = match op {
                        BinOp::Add => "add",
                        BinOp::Sub => "sub",
                        BinOp::Mul => "mul",
                        _ => unreachable!(),
                    };
                    self.emit(&format!("  {} = {} i64 {}, {}", r, inst, lr, rr));
                    Ok((r, Ty::Int))
                }
            }
        }
    }

    /// 比较运算（含浮点）
    fn gen_cmp(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr,
              scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        let (lr, lt) = self.gen_expr(lhs, scopes)?;
        let (rr, rt) = self.gen_expr(rhs, scopes)?;
        let is_float = lt.is_float() || rt.is_float();
        let r = self.fresh();
        if is_float {
            let lf = self.to_double(&lr, lt);
            let rf = self.to_double(&rr, rt);
            let pred = match op {
                BinOp::Eq => "oeq",
                BinOp::Neq => "one",
                BinOp::Lt => "olt",
                BinOp::Gt => "ogt",
                BinOp::Le => "ole",
                BinOp::Ge => "oge",
                _ => unreachable!(),
            };
            self.emit(&format!("  {} = fcmp {} double {}, {}", r, pred, lf, rf));
        } else {
            let pred = match op {
                BinOp::Eq => "eq",
                BinOp::Neq => "ne",
                BinOp::Lt => "slt",
                BinOp::Gt => "sgt",
                BinOp::Le => "sle",
                BinOp::Ge => "sge",
                _ => unreachable!(),
            };
            self.emit(&format!("  {} = icmp {} i64 {}, {}", r, pred, lr, rr));
        }
        let r2 = self.fresh();
        self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
        Ok((r2, Ty::Bool))
    }

    fn gen_string_parts(&mut self, expr: &Expr, scopes: &mut Vec<Scope>) -> Result<(String, String, String), String> {
        let (reg, ty) = self.gen_expr(expr, scopes)?;
        if ty != Ty::Str {
            return Err("LightWeb 路由参数必须是字符串".into());
        }
        let p = self.i64_to_ptr(&reg);
        let data = self.fresh();
        self.emit(&format!("  {} = call ptr @light_str_ptr(ptr {})", data, p));
        let len = self.fresh();
        self.emit(&format!("  {} = call i64 @light_str_len(ptr {})", len, p));
        Ok((reg, data, len))
    }

    fn gen_web_route(&mut self, args: &[Expr], scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        if args.len() != 3 {
            return Err("路由 需要方法、路径和处理器".into());
        }
        let method = match &args[0] {
            Expr::Str(method) => method.as_str(),
            _ => return Err("路由方法必须是字符串".into()),
        };
        let method_code = match method {
            "GET" => 0,
            "POST" => 1,
            _ => return Err("LightWeb 目前只支持 GET 和 POST".into()),
        };
        let handler = match &args[2] {
            Expr::Ident(name) => name,
            _ => return Err("路由处理器必须是函数名".into()),
        };
        let Some(handler_ty) = self.env.return_types.get(handler) else {
            return Err(format!("未找到路由处理器：{}", handler));
        };
        if Ty::from(handler_ty.clone()) != Ty::Str {
            return Err("LightWeb 处理器必须返回字符串".into());
        }
        let (method_reg, _, _) = self.gen_string_parts(&args[0], scopes)?;
        let (path_reg, path_data, path_len) = self.gen_string_parts(&args[1], scopes)?;
        let status = self.fresh();
        self.emit(&format!(
            "  {} = call i64 @light_web_register_v1(i32 {}, ptr {}, i64 {}, ptr @{})",
            status,
            method_code,
            path_data,
            path_len,
            Self::mangle(handler)
        ));
        self.free_value(&method_reg, &Ty::Str);
        self.free_value(&path_reg, &Ty::Str);
        Ok((status, Ty::Int))
    }

    fn gen_web_run(&mut self, args: &[Expr], scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        if args.len() != 1 {
            return Err("启动 需要一个端口参数".into());
        }
        let (port, ty) = self.gen_expr(&args[0], scopes)?;
        if ty != Ty::Int {
            return Err("启动端口必须是整数".into());
        }
        let result = self.fresh();
        self.emit(&format!("  {} = call i64 @light_web_run_v1(i64 {})", result, port));
        Ok((result, Ty::Int))
    }

    fn gen_web_request(&mut self, name: &str, args: &[Expr], scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        if args.is_empty() {
            return Err(format!("{} 需要请求参数", name));
        }
        let (request, _) = self.gen_expr(&args[0], scopes)?;
        if name == "请求头" {
            if args.len() != 2 {
                return Err("请求头 需要请求和名称".into());
            }
            let (name_reg, name_data, name_len) = self.gen_string_parts(&args[1], scopes)?;
            let result = self.fresh();
            self.emit(&format!(
                "  {} = call i64 @light_web_request_header_v1(i64 {}, ptr {}, i64 {})",
                result, request, name_data, name_len
            ));
            if !matches!(args[1], Expr::Ident(_)) {
                self.free_value(&name_reg, &Ty::Str);
            }
            Ok((result, Ty::Str))
        } else {
            if args.len() != 1 {
                return Err(format!("{} 只需要请求参数", name));
            }
            let function = match name {
                "请求方法" => "light_web_request_method_v1",
                "请求路径" => "light_web_request_path_v1",
                "请求体" => "light_web_request_body_v1",
                _ => return Err(format!("未知 LightWeb 请求函数：{}", name)),
            };
            let result = self.fresh();
            self.emit(&format!("  {} = call i64 @{}(i64 {})", result, function, request));
            Ok((result, Ty::Str))
        }
    }

    /// 函数调用与内置函数
    fn gen_call(&mut self, name: &str, args: &[Expr],
                scopes: &mut Vec<Scope>) -> Result<(String, Ty), String> {
        let name = name.rsplit('.').next().unwrap_or(name);
        match name {
            "路由" => return self.gen_web_route(args, scopes),
            "启动" => return self.gen_web_run(args, scopes),
            "请求方法" | "请求路径" | "请求体" | "请求头" => {
                return self.gen_web_request(name, args, scopes);
            }
            "等待全部" => {
                if !args.is_empty() {
                    return Err("等待全部 不需要参数".into());
                }
                let result = self.fresh();
                self.emit(&format!("  {} = call i64 @light_thread_join_all()", result));
                return Ok((result, Ty::Int));
            }
            _ => {}
        }
        // 用户自定义函数或外部声明优先于内置函数
        let is_user_fn = self.env.return_types.contains_key(name)
            || self.externs.contains(name);
        if !is_user_fn {
            match name {
            "范围" => {
                if args.len() != 1 { return Err("范围 需要 1 个参数".into()); }
                let (n, _) = self.gen_expr(&args[0], scopes)?;
                let ap = self.fresh();
                self.emit(&format!("  {} = call ptr @light_range(i64 {})", ap, n));
                let r = self.ptr_to_i64(&ap);
                return Ok((r, Ty::Array));
            }
            "长度" => {
                if args.len() != 1 { return Err("长度 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let p = self.i64_to_ptr(&v);
                let r = self.fresh();
                match ty {
                    Ty::Str => self.emit(&format!("  {} = call i64 @light_str_len(ptr {})", r, p)),
                    Ty::Array => self.emit(&format!("  {} = call i64 @light_array_len(ptr {})", r, p)),
                    _ => return Err("长度 只能作用于字符串或数组".into()),
                }
                // 临时字符串/数组用完即释放
                if !matches!(args[0], Expr::Ident(_)) {
                    match ty {
                        Ty::Str => self.emit(&format!("  call void @light_str_free(ptr {})", p)),
                        Ty::Array => self.emit(&format!("  call void @light_array_free(ptr {})", p)),
                        _ => {}
                    }
                }
                return Ok((r, Ty::Int));
            }
            "绝对值" => {
                if args.len() != 1 { return Err("绝对值 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let r = self.fresh();
                if ty.is_float() {
                    self.emit(&format!("  {} = call double @llvm.fabs.f64(double {})", r, v));
                    return Ok((r, Ty::Float));
                } else {
                    // int abs: 若 < 0 取负
                    let neg = self.fresh();
                    self.emit(&format!("  {} = sub i64 0, {}", neg, v));
                    let is_neg = self.fresh();
                    self.emit(&format!("  {} = icmp slt i64 {}, 0", is_neg, v));
                    self.emit(&format!("  {} = select i1 {}, i64 {}, i64 {}", r, is_neg, neg, v));
                    return Ok((r, Ty::Int));
                }
            }
            "最小" => {
                if args.len() != 2 { return Err("最小 需要 2 个参数".into()); }
                let (a, at) = self.gen_expr(&args[0], scopes)?;
                let (b, bt) = self.gen_expr(&args[1], scopes)?;
                let r = self.fresh();
                if at.is_float() || bt.is_float() {
                    let af = self.to_double(&a, at);
                    let bf = self.to_double(&b, bt);
                    self.emit(&format!("  {} = call double @llvm.minnum.f64(double {}, double {})", r, af, bf));
                    return Ok((r, Ty::Float));
                }
                let c = self.fresh();
                self.emit(&format!("  {} = icmp slt i64 {}, {}", c, a, b));
                self.emit(&format!("  {} = select i1 {}, i64 {}, i64 {}", r, c, a, b));
                return Ok((r, Ty::Int));
            }
            "最大" => {
                if args.len() != 2 { return Err("最大 需要 2 个参数".into()); }
                let (a, at) = self.gen_expr(&args[0], scopes)?;
                let (b, bt) = self.gen_expr(&args[1], scopes)?;
                let r = self.fresh();
                if at.is_float() || bt.is_float() {
                    let af = self.to_double(&a, at);
                    let bf = self.to_double(&b, bt);
                    self.emit(&format!("  {} = call double @llvm.maxnum.f64(double {}, double {})", r, af, bf));
                    return Ok((r, Ty::Float));
                }
                let c = self.fresh();
                self.emit(&format!("  {} = icmp sgt i64 {}, {}", c, a, b));
                self.emit(&format!("  {} = select i1 {}, i64 {}, i64 {}", r, c, a, b));
                return Ok((r, Ty::Int));
            }
            "求和" => {
                if args.len() != 1 { return Err("求和 需要 1 个数组参数".into()); }
                let (arr, ty) = self.gen_expr(&args[0], scopes)?;
                if ty != Ty::Array { return Err("求和 只能作用于数组".into()); }
                let ap = self.i64_to_ptr(&arr);
                let len = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_len(ptr {})", len, ap));
                let i_reg = self.fresh();
                self.emit(&format!("  {} = alloca i64", i_reg));
                self.emit(&format!("  store i64 0, ptr {}", i_reg));
                let sum_reg = self.fresh();
                self.emit(&format!("  {} = alloca i64", sum_reg));
                self.emit(&format!("  store i64 0, ptr {}", sum_reg));
                let head = self.fresh_label("sumhead");
                let body = self.fresh_label("sumbody");
                let end = self.fresh_label("sumend");
                self.emit(&format!("  br label %{}", head));
                self.emit(&format!("{}:", head));
                let iv = self.fresh();
                self.emit(&format!("  {} = load i64, ptr {}", iv, i_reg));
                let c = self.fresh();
                self.emit(&format!("  {} = icmp slt i64 {}, {}", c, iv, len));
                self.emit(&format!("  br i1 {}, label %{}, label %{}", c, body, end));
                self.emit(&format!("{}:", body));
                let elem = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_get(ptr {}, i64 {})", elem, ap, iv));
                let sv = self.fresh();
                self.emit(&format!("  {} = load i64, ptr {}", sv, sum_reg));
                let ns = self.fresh();
                self.emit(&format!("  {} = add i64 {}, {}", ns, sv, elem));
                self.emit(&format!("  store i64 {}, ptr {}", ns, sum_reg));
                let ni = self.fresh();
                self.emit(&format!("  {} = add i64 {}, 1", ni, iv));
                self.emit(&format!("  store i64 {}, ptr {}", ni, i_reg));
                self.emit(&format!("  br label %{}", head));
                self.emit(&format!("{}:", end));
                let r = self.fresh();
                self.emit(&format!("  {} = load i64, ptr {}", r, sum_reg));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                return Ok((r, Ty::Int));
            }
            "整数" => {
                if args.len() != 1 { return Err("整数 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                return match ty {
                    Ty::Float => {
                        let r = self.double_to_i64(&v);
                        Ok((r, Ty::Int))
                    }
                    Ty::Str => {
                        let p = self.i64_to_ptr(&v);
                        let r = self.fresh();
                        self.emit(&format!("  {} = call i64 @light_str_to_int(ptr {})", r, p));
                        if !matches!(args[0], Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_str_free(ptr {})", p));
                        }
                        Ok((r, Ty::Int))
                    }
                    _ => Ok((v, Ty::Int)),
                };
            }
            "浮点" => {
                if args.len() != 1 { return Err("浮点 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                return match ty {
                    Ty::Float => Ok((v, Ty::Float)),
                    Ty::Int | Ty::Bool => {
                        let r = self.to_double(&v, ty);
                        Ok((r, Ty::Float))
                    }
                    Ty::Str => {
                        let p = self.i64_to_ptr(&v);
                        let r = self.fresh();
                        self.emit(&format!("  {} = call double @light_str_to_float(ptr {})", r, p));
                        if !matches!(args[0], Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_str_free(ptr {})", p));
                        }
                        Ok((r, Ty::Float))
                    }
                    _ => Err("浮点 无法转换该类型".into()),
                };
            }
            "字符串" => {
                if args.len() != 1 { return Err("字符串 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let sp = self.fresh();
                match ty {
                    Ty::Int => self.emit(&format!("  {} = call ptr @light_int_to_str(i64 {})", sp, v)),
                    Ty::Bool => self.emit(&format!("  {} = call ptr @light_bool_to_str(i64 {})", sp, v)),
                    Ty::Float => self.emit(&format!("  {} = call ptr @light_float_to_str(double {})", sp, v)),
                    Ty::Str => {
                        return Ok((v, Ty::Str));
                    }
                    _ => return Err("字符串 无法转换该类型".into()),
                }
                let r = self.ptr_to_i64(&sp);
                return Ok((r, Ty::Str));
            }
            "输入" => {
                if !args.is_empty() { return Err("输入 不需要参数".into()); }
                let sp = self.fresh();
                self.emit(&format!("  {} = call ptr @light_input()", sp));
                let r = self.ptr_to_i64(&sp);
                return Ok((r, Ty::Str));
            }
            "追加" => {
                if args.len() != 2 { return Err("追加 需要 2 个参数：数组与值".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                if aty != Ty::Array { return Err("追加 第一个参数必须是数组".into()); }
                let (val, vty) = self.gen_expr(&args[1], scopes)?;
                let val_i = if vty.is_float() { self.double_to_i64(&val) } else { val };
                let ap = self.i64_to_ptr(&arr);
                self.emit(&format!("  call void @light_array_push(ptr {}, i64 {})", ap, val_i));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                let r = self.fresh();
                self.emit(&format!("  {} = add i64 0, 0", r));
                return Ok((r, Ty::None));
            }
            "弹出" => {
                if args.len() != 1 { return Err("弹出 需要 1 个数组参数".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                if aty != Ty::Array { return Err("弹出 参数必须是数组".into()); }
                let ap = self.i64_to_ptr(&arr);
                let r = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_pop(ptr {})", r, ap));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                return Ok((r, Ty::Int));
            }
            // ---- 数学模块 ----
            "正弦" => {
                if args.len() != 1 { return Err("正弦 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_sin(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "余弦" => {
                if args.len() != 1 { return Err("余弦 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_cos(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "正切" => {
                if args.len() != 1 { return Err("正切 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_tan(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "平方根" => {
                if args.len() != 1 { return Err("平方根 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_sqrt(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "对数" => {
                if args.len() != 1 { return Err("对数 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_log(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "对数10" => {
                if args.len() != 1 { return Err("对数10 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_log10(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "幂" => {
                if args.len() != 2 { return Err("幂 需要 2 个参数".into()); }
                let (a, at) = self.gen_expr(&args[0], scopes)?;
                let (b, bt) = self.gen_expr(&args[1], scopes)?;
                let af = self.to_double(&a, at);
                let bf = self.to_double(&b, bt);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_pow(double {}, double {})", r, af, bf));
                return Ok((r, Ty::Float));
            }
            "圆周率" => {
                if !args.is_empty() { return Err("圆周率 不需要参数".into()); }
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_pi()", r));
                return Ok((r, Ty::Float));
            }
            "自然常数" => {
                if !args.is_empty() { return Err("自然常数 不需要参数".into()); }
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_e()", r));
                return Ok((r, Ty::Float));
            }
            "向上取整" => {
                if args.len() != 1 { return Err("向上取整 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_ceil(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "向下取整" => {
                if args.len() != 1 { return Err("向下取整 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                let vf = self.to_double(&v, ty);
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_floor(double {})", r, vf));
                return Ok((r, Ty::Float));
            }
            "随机" => {
                if !args.is_empty() { return Err("随机 不需要参数".into()); }
                let r = self.fresh();
                self.emit(&format!("  {} = call double @light_math_random()", r));
                return Ok((r, Ty::Float));
            }
            "随机整数" => {
                if args.len() != 2 { return Err("随机整数 需要 2 个参数".into()); }
                let (a, _) = self.gen_expr(&args[0], scopes)?;
                let (b, _) = self.gen_expr(&args[1], scopes)?;
                let r = self.fresh();
                self.emit(&format!("  {} = call i64 @light_math_random_int(i64 {}, i64 {})", r, a, b));
                return Ok((r, Ty::Int));
            }
            // ---- 字符串模块 ----
            "查找" => {
                if args.len() != 2 { return Err("查找 需要 2 个参数".into()); }
                let (s, sty) = self.gen_expr(&args[0], scopes)?;
                let (sub, _) = self.gen_expr(&args[1], scopes)?;
                if sty != Ty::Str { return Err("查找 第一个参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&s);
                let subp = self.i64_to_ptr(&sub);
                let r = self.fresh();
                self.emit(&format!("  {} = call i64 @light_str_find(ptr {}, ptr {})", r, sp, subp));
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", subp));
                }
                return Ok((r, Ty::Int));
            }
            "包含" => {
                if args.len() != 2 { return Err("包含 需要 2 个参数".into()); }
                let (s, sty) = self.gen_expr(&args[0], scopes)?;
                if sty == Ty::Str {
                    let (sub, _) = self.gen_expr(&args[1], scopes)?;
                    let sp = self.i64_to_ptr(&s);
                    let subp = self.i64_to_ptr(&sub);
                    let r = self.fresh();
                    self.emit(&format!("  {} = call i1 @light_str_contains(ptr {}, ptr {})", r, sp, subp));
                    let r2 = self.fresh();
                    self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
                    if !matches!(args[1], Expr::Ident(_)) {
                        self.emit(&format!("  call void @light_str_free(ptr {})", subp));
                    }
                    return Ok((r2, Ty::Bool));
                } else if sty == Ty::Array {
                    let (val, vty) = self.gen_expr(&args[1], scopes)?;
                    let val_i = if vty.is_float() { self.double_to_i64(&val) } else { val };
                    let ap = self.i64_to_ptr(&s);
                    let r = self.fresh();
                    self.emit(&format!("  {} = call i1 @light_array_contains(ptr {}, i64 {})", r, ap, val_i));
                    let r2 = self.fresh();
                    self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
                    return Ok((r2, Ty::Bool));
                } else {
                    return Err("包含 只能作用于字符串或数组".into());
                }
            }
            "替换" => {
                if args.len() != 3 { return Err("替换 需要 3 个参数".into()); }
                let (s, sty) = self.gen_expr(&args[0], scopes)?;
                let (old, _) = self.gen_expr(&args[1], scopes)?;
                let (new, _) = self.gen_expr(&args[2], scopes)?;
                if sty != Ty::Str { return Err("替换 第一个参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&s);
                let oldp = self.i64_to_ptr(&old);
                let newp = self.i64_to_ptr(&new);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_replace(ptr {}, ptr {}, ptr {})", r, sp, oldp, newp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", oldp));
                }
                if !matches!(args[2], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", newp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "分割" => {
                if args.len() != 2 { return Err("分割 需要 2 个参数".into()); }
                let (s, sty) = self.gen_expr(&args[0], scopes)?;
                let (sep, _) = self.gen_expr(&args[1], scopes)?;
                if sty != Ty::Str { return Err("分割 第一个参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&s);
                let sepp = self.i64_to_ptr(&sep);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_split(ptr {}, ptr {})", r, sp, sepp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sepp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Array));
            }
            "合并" => {
                if args.len() != 2 { return Err("合并 需要 2 个参数（数组, 分隔符）".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                let (sep, _) = self.gen_expr(&args[1], scopes)?;
                if aty != Ty::Array { return Err("合并 第一个参数必须是数组".into()); }
                let ap = self.i64_to_ptr(&arr);
                let sepp = self.i64_to_ptr(&sep);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_join(ptr {}, ptr {})", r, ap, sepp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sepp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "大写" => {
                if args.len() != 1 { return Err("大写 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                if ty != Ty::Str { return Err("大写 参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&v);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_upper(ptr {})", r, sp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "小写" => {
                if args.len() != 1 { return Err("小写 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                if ty != Ty::Str { return Err("小写 参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&v);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_lower(ptr {})", r, sp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "去空白" => {
                if args.len() != 1 { return Err("去空白 需要 1 个参数".into()); }
                let (v, ty) = self.gen_expr(&args[0], scopes)?;
                if ty != Ty::Str { return Err("去空白 参数必须是字符串".into()); }
                let sp = self.i64_to_ptr(&v);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_str_trim(ptr {})", r, sp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "切片" => {
                if args.len() != 3 { return Err("切片 需要 3 个参数（字符串/数组, 起始, 结束）".into()); }
                let (s, sty) = self.gen_expr(&args[0], scopes)?;
                let (start, _) = self.gen_expr(&args[1], scopes)?;
                let (end, _) = self.gen_expr(&args[2], scopes)?;
                match sty {
                    Ty::Str => {
                        let sp = self.i64_to_ptr(&s);
                        let r = self.fresh();
                        self.emit(&format!("  {} = call ptr @light_str_substr(ptr {}, i64 {}, i64 {})", r, sp, start, end));
                        if !matches!(args[0], Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_str_free(ptr {})", sp));
                        }
                        let rr = self.ptr_to_i64(&r);
                        return Ok((rr, Ty::Str));
                    }
                    Ty::Array => {
                        let ap = self.i64_to_ptr(&s);
                        let r = self.fresh();
                        self.emit(&format!("  {} = call ptr @light_array_slice(ptr {}, i64 {}, i64 {})", r, ap, start, end));
                        if !matches!(args[0], Expr::Ident(_)) {
                            self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                        }
                        let rr = self.ptr_to_i64(&r);
                        return Ok((rr, Ty::Array));
                    }
                    _ => return Err("切片 只能作用于字符串或数组".into()),
                }
            }
            // ---- 数组扩展模块 ----
            "索引" => {
                if args.len() != 2 { return Err("索引 需要 2 个参数（数组, 值）".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                let (val, vty) = self.gen_expr(&args[1], scopes)?;
                if aty != Ty::Array { return Err("索引 第一个参数必须是数组".into()); }
                let val_i = if vty.is_float() { self.double_to_i64(&val) } else { val };
                let ap = self.i64_to_ptr(&arr);
                let r = self.fresh();
                self.emit(&format!("  {} = call i64 @light_array_index(ptr {}, i64 {})", r, ap, val_i));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                return Ok((r, Ty::Int));
            }
            "反转" => {
                if args.len() != 1 { return Err("反转 需要 1 个数组参数".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                if aty != Ty::Array { return Err("反转 参数必须是数组".into()); }
                let ap = self.i64_to_ptr(&arr);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_array_reverse(ptr {})", r, ap));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Array));
            }
            "排序" => {
                if args.len() != 1 { return Err("排序 需要 1 个数组参数".into()); }
                let (arr, aty) = self.gen_expr(&args[0], scopes)?;
                if aty != Ty::Array { return Err("排序 参数必须是数组".into()); }
                let ap = self.i64_to_ptr(&arr);
                self.emit(&format!("  call void @light_array_sort(ptr {})", ap));
                return Ok((arr, Ty::Array));
            }
            "连接" => {
                if args.len() != 2 { return Err("连接 需要 2 个数组参数".into()); }
                let (a, aty) = self.gen_expr(&args[0], scopes)?;
                let (b, bty) = self.gen_expr(&args[1], scopes)?;
                if aty != Ty::Array || bty != Ty::Array { return Err("连接 参数必须是数组".into()); }
                let ap = self.i64_to_ptr(&a);
                let bp = self.i64_to_ptr(&b);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_array_concat(ptr {}, ptr {})", r, ap, bp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", ap));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_array_free(ptr {})", bp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Array));
            }
            // ---- 文件模块 ----
            "读取文件" => {
                if args.len() != 1 { return Err("读取文件 需要 1 个路径参数".into()); }
                let (path, pty) = self.gen_expr(&args[0], scopes)?;
                if pty != Ty::Str { return Err("读取文件 参数必须是字符串".into()); }
                let pp = self.i64_to_ptr(&path);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_file_read(ptr {})", r, pp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", pp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "写入文件" => {
                if args.len() != 2 { return Err("写入文件 需要 2 个参数（路径, 内容）".into()); }
                let (path, pty) = self.gen_expr(&args[0], scopes)?;
                let (content, cty) = self.gen_expr(&args[1], scopes)?;
                if pty != Ty::Str || cty != Ty::Str { return Err("写入文件 参数必须是字符串".into()); }
                let pp = self.i64_to_ptr(&path);
                let cp = self.i64_to_ptr(&content);
                let r = self.fresh();
                self.emit(&format!("  {} = call i1 @light_file_write(ptr {}, ptr {})", r, pp, cp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", pp));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", cp));
                }
                let r2 = self.fresh();
                self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
                return Ok((r2, Ty::Bool));
            }
            "追加文件" => {
                if args.len() != 2 { return Err("追加文件 需要 2 个参数（路径, 内容）".into()); }
                let (path, pty) = self.gen_expr(&args[0], scopes)?;
                let (content, cty) = self.gen_expr(&args[1], scopes)?;
                if pty != Ty::Str || cty != Ty::Str { return Err("追加文件 参数必须是字符串".into()); }
                let pp = self.i64_to_ptr(&path);
                let cp = self.i64_to_ptr(&content);
                let r = self.fresh();
                self.emit(&format!("  {} = call i1 @light_file_append(ptr {}, ptr {})", r, pp, cp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", pp));
                }
                if !matches!(args[1], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", cp));
                }
                let r2 = self.fresh();
                self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
                return Ok((r2, Ty::Bool));
            }
            "文件存在" => {
                if args.len() != 1 { return Err("文件存在 需要 1 个路径参数".into()); }
                let (path, pty) = self.gen_expr(&args[0], scopes)?;
                if pty != Ty::Str { return Err("文件存在 参数必须是字符串".into()); }
                let pp = self.i64_to_ptr(&path);
                let r = self.fresh();
                self.emit(&format!("  {} = call i1 @light_file_exists(ptr {})", r, pp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", pp));
                }
                let r2 = self.fresh();
                self.emit(&format!("  {} = zext i1 {} to i64", r2, r));
                return Ok((r2, Ty::Bool));
            }
            // ---- 系统模块 ----
            "执行" => {
                if args.len() != 1 { return Err("执行 需要 1 个命令参数".into()); }
                let (cmd, cty) = self.gen_expr(&args[0], scopes)?;
                if cty != Ty::Str { return Err("执行 参数必须是字符串".into()); }
                let cp = self.i64_to_ptr(&cmd);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_sys_exec(ptr {})", r, cp));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", cp));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "环境变量" => {
                if args.len() != 1 { return Err("环境变量 需要 1 个名称参数".into()); }
                let (name, nty) = self.gen_expr(&args[0], scopes)?;
                if nty != Ty::Str { return Err("环境变量 参数必须是字符串".into()); }
                let np = self.i64_to_ptr(&name);
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_sys_getenv(ptr {})", r, np));
                if !matches!(args[0], Expr::Ident(_)) {
                    self.emit(&format!("  call void @light_str_free(ptr {})", np));
                }
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            "退出" => {
                if args.len() != 1 { return Err("退出 需要 1 个退出码参数".into()); }
                let (code, _) = self.gen_expr(&args[0], scopes)?;
                self.emit(&format!("  call void @light_sys_exit(i64 {})", code));
                return Ok(("0".to_string(), Ty::None));
            }
            "字符" => {
                if args.len() != 1 { return Err("字符 需要 1 个参数（字节值）".into()); }
                let (v, _) = self.gen_expr(&args[0], scopes)?;
                let r = self.fresh();
                self.emit(&format!("  {} = call ptr @light_byte_to_char(i64 {})", r, v));
                let rr = self.ptr_to_i64(&r);
                return Ok((rr, Ty::Str));
            }
            _ => {}
            }
        }

        // 普通函数调用
        let mut arg_vals = Vec::new();
        let mut arg_tys = Vec::new();
        for a in args {
            let (r, t) = self.gen_expr(a, scopes)?;
            // 浮点参数 bitcast 为 i64
            let v = if t.is_float() {
                let bc = self.fresh();
                self.emit(&format!("  {} = bitcast double {} to i64", bc, r));
                bc
            } else {
                r
            };
            arg_vals.push(v);
            arg_tys.push(t);
        }
        // move 语义：把拥有堆资源的实参所有权转移给被调函数
        let is_user_fn = self.env.return_types.contains_key(name);
        if is_user_fn {
            for a in args {
                if let Expr::Ident(n) = a {
                    Self::mark_moved(scopes, n);
                }
            }
        }
        let arg_list = arg_vals.iter().map(|r| format!("i64 {}", r)).collect::<Vec<_>>().join(", ");
        let r = self.fresh();
        let callee = if self.externs.contains(name) {
            name.to_string()
        } else {
            Self::mangle(name)
        };
        self.emit(&format!("  {} = call i64 @{}({})", r, callee, arg_list));
        let ret_ty: Ty = self.env.return_types.get(name).cloned()
            .map(|t| t.into())
            .unwrap_or(Ty::Int);
        // 浮点返回：bitcast 回 double
        if ret_ty.is_float() {
            let f = self.fresh();
            self.emit(&format!("  {} = bitcast i64 {} to double", f, r));
            Ok((f, Ty::Float))
        } else {
            Ok((r, ret_ty))
        }
    }
}

/// 把 f64 格式化为 LLVM 浮点常量（使用 IEEE-754 十六进制表示，精确且通用）
fn format_float_const(n: f64) -> String {
    format!("0x{:016x}", n.to_bits())
}
