use crate::ast::{
    ClassMember, Expression, ExpressionKind, FunctionBody, Literal, Statement, StatementKind,
    StringPart,
};
use anyhow::{bail, Result};
use matchbox_vm::types::{
    box_string::BoxString, BxClass, BxCompiledFunction, BxInterface, Constant,
};
use matchbox_vm::vm::opcode::op;
use matchbox_vm::Chunk;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
struct Local {
    name: String,
    depth: i32,
}

pub struct Compiler {
    pub chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: i32,
    is_class: bool,
    imports: HashMap<String, String>, // Alias -> Full dotted path
    pub module_paths: HashMap<String, PathBuf>, // "bxmodules.{name}" -> physical PathBuf
    current_line: u32,
    pub is_repl: bool,
    continue_patches: Vec<Vec<usize>>,
    break_patches: Vec<Vec<usize>>,
    loop_locals: Vec<usize>,
    class_methods: HashSet<String>, // Method names of the current class being compiled
    /// Directory to resolve unqualified source paths against (e.g. sibling interfaces).
    source_dir: Option<PathBuf>,
    /// Tracks which class names (lowercased) are known to define an init() method.
    /// Propagated to sub-compilers so that `new` expressions in functions/methods
    /// can still determine whether to emit an `init` invocation.
    class_has_init_map: HashMap<String, bool>,
}

impl Compiler {
    pub fn new(filename: &str) -> Self {
        Compiler {
            chunk: Chunk::new(filename),
            locals: Vec::new(),
            scope_depth: 0,
            is_class: false,
            imports: HashMap::new(),
            module_paths: HashMap::new(),
            current_line: 0,
            is_repl: false,
            continue_patches: Vec::new(),
            break_patches: Vec::new(),
            loop_locals: Vec::new(),
            class_methods: HashSet::new(),
            source_dir: None,
            class_has_init_map: HashMap::new(),
        }
    }

    pub fn with_chunk(chunk: Chunk) -> Self {
        Compiler {
            chunk,
            locals: Vec::new(),
            scope_depth: 0,
            is_class: false,
            imports: HashMap::new(),
            module_paths: HashMap::new(),
            current_line: 0,
            is_repl: false,
            continue_patches: Vec::new(),
            break_patches: Vec::new(),
            loop_locals: Vec::new(),
            class_methods: HashSet::new(),
            source_dir: None,
            class_has_init_map: HashMap::new(),
        }
    }

    pub fn compile(mut self, ast: &[Statement], source: &str) -> Result<Chunk> {
        self.chunk.source = source.to_string();
        let len = ast.len();
        for (i, stmt) in ast.iter().enumerate() {
            let is_last = i == len - 1;
            self.compile_statement(stmt, is_last)?;
        }
        self.chunk.emit0(op::RETURN, self.current_line as u32);
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<()> {
        self.current_line = stmt.line as u32;
        match &stmt.kind {
            StatementKind::Import { path, alias } => {
                let dotted_path = if let Some(pos) = path.find(':') {
                    &path[pos + 1..]
                } else {
                    path.as_str()
                };
                let original_alias = if let Some(a) = alias {
                    a.clone()
                } else {
                    dotted_path.split('.').last().unwrap().to_string()
                };
                let resolved_alias = original_alias.to_lowercase();

                if path.to_lowercase().starts_with("js:") {
                    let js_path = &path[3..];
                    let segments: Vec<&str> = js_path.split('.').collect();

                    let js_global = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("js")));
                    self.chunk
                        .emit1(op::GET_GLOBAL, js_global, self.current_line);

                    for segment in segments {
                        let idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new(segment)));
                        self.chunk.emit1(op::MEMBER, idx, self.current_line);
                    }

                    let alias_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(&original_alias)));
                    self.chunk
                        .emit1(op::SET_GLOBAL_POP, alias_idx, self.current_line);
                }

                self.imports.insert(resolved_alias, path.clone());
                Ok(())
            }
            StatementKind::ClassDecl {
                name,
                extends,
                accessors,
                implements,
                members,
            } => {
                let mut constructor_compiler = Compiler::with_chunk(self.chunk.new_sub_chunk());
                constructor_compiler.is_class = true;
                constructor_compiler.scope_depth = 1;
                constructor_compiler.imports = self.imports.clone();
                constructor_compiler.module_paths = self.module_paths.clone();
                constructor_compiler.source_dir = self.source_dir.clone();
                constructor_compiler.current_line = stmt.line as u32;
                constructor_compiler.class_has_init_map = self.class_has_init_map.clone();

                let mut methods = HashMap::new();
                let mut properties = Vec::new();

                // Collect method names so unqualified calls inside methods resolve correctly.
                let mut class_method_names = HashSet::new();
                for member in members.iter() {
                    if let ClassMember::Statement(inner_stmt) = member {
                        if let StatementKind::FunctionDecl {
                            name: func_name, ..
                        } = &inner_stmt.kind
                        {
                            class_method_names.insert(func_name.to_lowercase());
                        }
                    }
                }
                for member in members {
                    match member {
                        ClassMember::Property(prop_name) => {
                            properties.push(prop_name.clone());
                            let null_idx = constructor_compiler.chunk.add_constant(Constant::Null);
                            constructor_compiler.chunk.emit1(
                                op::CONSTANT,
                                null_idx,
                                stmt.line as u32,
                            );
                            let name_idx = constructor_compiler
                                .chunk
                                .add_constant(Constant::String(BoxString::new(prop_name.as_str())));
                            constructor_compiler.chunk.emit1(
                                op::SET_PRIVATE,
                                name_idx as u32,
                                stmt.line as u32,
                            );
                            constructor_compiler.chunk.emit0(op::POP, stmt.line as u32);
                        }
                        ClassMember::Statement(inner_stmt) => match &inner_stmt.kind {
                            StatementKind::FunctionDecl {
                                name: func_name,
                                attributes: _,
                                access_modifier: _,
                                return_type: _,
                                params,
                                body,
                            } => {
                                if let FunctionBody::Abstract = body {
                                    bail!("Abstract functions only allowed in interfaces");
                                }
                                let mut method_compiler =
                                    Compiler::with_chunk(self.chunk.new_sub_chunk());
                                method_compiler.is_class = true;
                                method_compiler.imports = self.imports.clone();
                                method_compiler.module_paths = self.module_paths.clone();
                                method_compiler.source_dir = self.source_dir.clone();
                                method_compiler.current_line = inner_stmt.line as u32;
                                method_compiler.class_methods = class_method_names.clone();
                                method_compiler.class_has_init_map = self.class_has_init_map.clone();
                                let mut func =
                                    method_compiler.compile_function(&func_name, &params, &body)?;
                                methods.insert(func_name.to_lowercase(), func);
                            }
                            _ => {
                                constructor_compiler.compile_statement(inner_stmt, false)?;
                            }
                        },
                    }
                }

                if *accessors {
                    for prop in &properties {
                        if prop.is_empty() {
                            continue;
                        }
                        let capitalized = format!("{}{}", &prop[..1].to_uppercase(), &prop[1..]);

                        // Getter: getProp()
                        let getter_name = format!("get{}", capitalized);
                        if !methods.contains_key(&getter_name.to_lowercase()) {
                            let mut getter_chunk = Chunk::default();
                            getter_chunk.filename = self.chunk.filename.clone();
                            let name_idx = getter_chunk
                                .add_constant(Constant::String(BoxString::new(prop.as_str())));
                            getter_chunk.emit1(op::GET_PRIVATE, name_idx as u32, stmt.line as u32);
                            getter_chunk.emit0(op::RETURN, stmt.line as u32);

                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, getter_name),
                                arity: 0,
                                min_arity: 0,
                                params: Vec::new(),
                                captured_receiver: None,
                                chunk: getter_chunk,
                            };
                            methods.insert(getter_name.to_lowercase(), func);
                        }

                        // Setter: setProp(val)
                        let setter_name = format!("set{}", capitalized);
                        if !methods.contains_key(&setter_name.to_lowercase()) {
                            let mut setter_chunk = Chunk::default();
                            setter_chunk.filename = self.chunk.filename.clone();
                            setter_chunk.emit1(op::GET_LOCAL, 0, stmt.line as u32);
                            let name_idx = setter_chunk
                                .add_constant(Constant::String(BoxString::new(prop.as_str())));
                            setter_chunk.emit1(op::SET_PRIVATE, name_idx as u32, stmt.line as u32);
                            setter_chunk.emit0(op::RETURN, stmt.line as u32);

                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, setter_name),
                                arity: 1,
                                min_arity: 1,
                                params: vec!["val".to_string()],
                                captured_receiver: None,
                                chunk: setter_chunk,
                            };
                            methods.insert(setter_name.to_lowercase(), func);
                        }
                    }
                }

                // Handle Interfaces (Traits and Contracts)
                for iface_name in implements {
                    let iface_val =
                        if let Some(alias_path) = self.imports.get(&iface_name.to_lowercase()) {
                            let path = alias_path.clone();
                            self.load_interface_from_path(&path)?
                        } else if let Some(found) = self.chunk.constants.iter().find_map(|c| {
                            if let Constant::Interface(i) = c {
                                if i.name.to_lowercase() == iface_name.to_lowercase() {
                                    return Some(Constant::Interface(i.clone()));
                                }
                            }
                            None
                        }) {
                            found
                        } else {
                            // Last-ditch: try to load the interface from disk (e.g. sibling file).
                            self.load_interface_from_path(iface_name)?
                        };

                    if let Constant::Interface(iface) = iface_val {
                        for (method_name, method_opt) in &iface.methods {
                            if !methods.contains_key(method_name) {
                                if let Some(default_impl) = method_opt {
                                    methods.insert(method_name.clone(), default_impl.clone());
                                } else {
                                    bail!("Class {} must implement abstract method {} from interface {}", name, method_name, iface.name);
                                }
                            }
                        }
                    }
                }

                constructor_compiler
                    .chunk
                    .emit0(op::RETURN, stmt.line as u32);

                let constructor = BxCompiledFunction {
                    name: format!("{}.constructor", name),
                    arity: 0,
                    min_arity: 0,
                    params: Vec::new(),
                    captured_receiver: None,
                    chunk: constructor_compiler.chunk,
                };

                let has_init = methods.keys().any(|n| n.eq_ignore_ascii_case("init"));
                self.class_has_init_map
                    .insert(name.to_lowercase(), has_init);

                let class = BxClass {
                    name: name.clone(),
                    extends: extends.as_ref().map(|s| s.to_lowercase()),
                    implements: implements.iter().map(|s| s.to_lowercase()).collect(),
                    constructor,
                    methods: methods.into_iter().collect(),
                };

                let class_idx = self.chunk.add_constant(Constant::Class(class));
                self.chunk.emit1(op::CONSTANT, class_idx, stmt.line as u32);
                let name_idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(name.as_str())));
                self.chunk
                    .emit1(op::DEFINE_GLOBAL, name_idx as u32, stmt.line as u32);
                Ok(())
            }
            StatementKind::InterfaceDecl { name, members } => {
                let mut methods = HashMap::new();
                for member in members {
                    if let StatementKind::FunctionDecl {
                        name: func_name,
                        attributes: _,
                        access_modifier: _,
                        return_type: _,
                        params,
                        body,
                    } = &member.kind
                    {
                        let method = if let FunctionBody::Abstract = body {
                            None
                        } else {
                            let mut method_compiler =
                                Compiler::with_chunk(self.chunk.new_sub_chunk());
                            method_compiler.is_class = true;
                            method_compiler.imports = self.imports.clone();
                            method_compiler.module_paths = self.module_paths.clone();
                            method_compiler.source_dir = self.source_dir.clone();
                            method_compiler.current_line = member.line;
                            let func = method_compiler.compile_function(func_name, params, body)?;
                            Some(func)
                        };
                        methods.insert(func_name.to_lowercase(), method);
                    } else {
                        bail!("Only function declarations allowed in interfaces");
                    }
                }

                let iface = BxInterface {
                    name: name.clone(),
                    methods: methods.into_iter().collect(),
                };

                let iface_idx = self.chunk.add_constant(Constant::Interface(iface));
                self.chunk.emit1(op::CONSTANT, iface_idx, stmt.line as u32);
                let name_idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(name.as_str())));

                self.chunk
                    .emit1(op::DEFINE_GLOBAL, name_idx as u32, stmt.line as u32);
                Ok(())
            }
            StatementKind::Expression(expr) => {
                if self.is_repl && is_last {
                    self.compile_expression(expr)?;
                } else {
                    self.compile_expression_as_statement(expr)?;
                }
                Ok(())
            }
            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.compile_expression(e)?;
                } else {
                    let null_idx = self.chunk.add_constant(Constant::Null);
                    self.chunk.emit1(op::CONSTANT, null_idx, stmt.line as u32);
                }
                self.chunk.emit0(op::RETURN, stmt.line as u32);
                Ok(())
            }
            StatementKind::Throw(expr) => {
                if let Some(e) = expr {
                    self.compile_expression(e)?;
                } else {
                    let null_idx = self.chunk.add_constant(Constant::Null);
                    self.chunk.emit1(op::CONSTANT, null_idx, stmt.line as u32);
                }
                self.chunk.emit0(op::THROW, stmt.line as u32);
                Ok(())
            }
            StatementKind::Continue => {
                // Pop any locals added within the loop body
                if let Some(target_local_count) = self.loop_locals.last() {
                    let current_local_count = self.locals.len();
                    if current_local_count > *target_local_count {
                        for _ in 0..(current_local_count - *target_local_count) {
                            self.chunk.emit0(op::POP, stmt.line as u32);
                        }
                    }
                }

                let jump_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, stmt.line as u32);
                if let Some(patches) = self.continue_patches.last_mut() {
                    patches.push(jump_idx);
                } else {
                    bail!("'continue' used outside of a loop");
                }
                Ok(())
            }
            StatementKind::Break => {
                // Pop any locals added within the loop/switch body
                if let Some(target_local_count) = self.loop_locals.last() {
                    let current_local_count = self.locals.len();
                    if current_local_count > *target_local_count {
                        for _ in 0..(current_local_count - *target_local_count) {
                            self.chunk.emit0(op::POP, stmt.line as u32);
                        }
                    }
                }

                let jump_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, stmt.line as u32);
                if let Some(patches) = self.break_patches.last_mut() {
                    patches.push(jump_idx);
                } else {
                    bail!("'break' used outside of a loop or switch");
                }
                Ok(())
            }
            StatementKind::TryCatch {
                try_branch,
                catches,
                finally_branch,
            } => {
                let push_handler_idx = self.chunk.code.len();
                self.chunk.emit1(op::PUSH_HANDLER, 0, stmt.line as u32);

                self.begin_scope();
                for s in try_branch {
                    self.compile_statement(s, false)?;
                }
                self.end_scope();

                self.chunk.emit0(op::POP_HANDLER, stmt.line as u32);

                let jump_to_finally_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, stmt.line as u32);

                let catch_target = self.chunk.code.len();
                let offset = catch_target - push_handler_idx - 1;
                self.chunk.code[push_handler_idx] =
                    op::PUSH_HANDLER as u32 | ((offset as u32) << 8);

                if !catches.is_empty() {
                    let first_catch = &catches[0];
                    self.begin_scope();
                    self.add_local(first_catch.exception_var.clone());
                    for s in &first_catch.body {
                        self.compile_statement(s, false)?;
                    }
                    self.end_scope();
                } else {
                    self.chunk.emit0(op::THROW, stmt.line as u32);
                }

                let finally_target = self.chunk.code.len();
                let jump_offset = finally_target - jump_to_finally_idx - 1;
                self.chunk.code[jump_to_finally_idx] =
                    op::JUMP as u32 | ((jump_offset as u32) << 8);

                if let Some(finally_stmts) = finally_branch {
                    self.begin_scope();
                    for s in finally_stmts {
                        self.compile_statement(s, false)?;
                    }
                    self.end_scope();
                }

                Ok(())
            }
            StatementKind::VariableDecl { name, value } => {
                self.compile_expression(value)?;
                if self.scope_depth > 0 {
                    self.add_local(name.clone());
                } else {
                    if self.is_repl && is_last {
                        self.chunk.emit0(op::DUP, stmt.line as u32);
                    }
                    let name_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(name.as_str())));
                    self.chunk
                        .emit1(op::DEFINE_GLOBAL, name_idx as u32, stmt.line as u32);
                }
                Ok(())
            }
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut optimized = false;
                let mut jump_if_false_idx = 0;

                if let ExpressionKind::Binary {
                    left,
                    operator,
                    right,
                } = &condition.kind
                {
                    if *operator == "==" {
                        if let (ExpressionKind::Identifier(name), ExpressionKind::Literal(lit)) =
                            (&left.kind, &right.kind)
                        {
                            if let Some(slot) = self.resolve_local(name) {
                                // Only apply LOCAL_JUMP_IF_NE_CONST for value-typed constants
                                // (numbers, bools, null). Strings are heap-allocated GC objects
                                // and require content comparison — bitwise BxValue equality fails
                                // because the argument and the constant are different allocations.
                                let const_idx = match lit {
                                    Literal::Number(n) => {
                                        Some(self.chunk.add_constant(Constant::Number(*n)))
                                    }
                                    Literal::Boolean(b) => {
                                        Some(self.chunk.add_constant(Constant::Boolean(*b)))
                                    }
                                    Literal::Null => Some(self.chunk.add_constant(Constant::Null)),
                                    _ => None,
                                };

                                if let Some(c_idx) = const_idx {
                                    jump_if_false_idx = self.chunk.code.len();
                                    self.chunk.emit3(
                                        op::LOCAL_JUMP_IF_NE_CONST,
                                        slot as u32,
                                        c_idx as u32,
                                        0,
                                        condition.line as u32,
                                    );
                                    optimized = true;
                                }
                            }
                        }
                    }
                }

                if !optimized {
                    self.compile_expression(condition)?;
                    jump_if_false_idx = self.chunk.code.len();
                    self.chunk.emit1(op::JUMP_IF_FALSE, 0, stmt.line as u32);

                    // True branch: pop truthy result
                    self.chunk.emit0(op::POP, stmt.line as u32);
                }

                self.begin_scope();
                for stmt in then_branch {
                    self.compile_statement(stmt, false)?;
                }
                self.end_scope();

                let jump_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, stmt.line as u32);

                let false_target = self.chunk.code.len();
                let offset = if optimized {
                    false_target - jump_if_false_idx - 3 // 3-word LOCAL_JUMP_IF_NE_CONST
                } else {
                    false_target - jump_if_false_idx - 1 // 1-word JUMP_IF_FALSE
                };

                if optimized {
                    self.chunk.code[jump_if_false_idx + 2] = offset as u32;
                } else {
                    self.chunk.code[jump_if_false_idx] =
                        op::JUMP_IF_FALSE as u32 | ((offset as u32) << 8);

                    // False branch: pop falsy result
                    self.chunk.emit0(op::POP, stmt.line as u32);
                }

                if let Some(else_stmts) = else_branch {
                    self.begin_scope();
                    for stmt in else_stmts {
                        self.compile_statement(stmt, false)?;
                    }
                    self.end_scope();
                }

                let end_target = self.chunk.code.len();
                let jump_offset = end_target - jump_idx - 1;
                self.chunk.code[jump_idx] = op::JUMP as u32 | ((jump_offset as u32) << 8);

                Ok(())
            }
            StatementKind::ForClassic {
                init,
                condition,
                update,
                body,
            } => {
                self.begin_scope();
                if let Some(init_stmt) = init {
                    self.compile_statement(init_stmt, false)?;
                }

                let loop_start = self.chunk.code.len();
                let mut exit_jump = None;
                let mut optimized_condition = None;

                if let Some(cond_expr) = condition {
                    // Try to optimize i < CONST
                    let mut handled = false;
                    if let ExpressionKind::Binary {
                        left,
                        operator,
                        right,
                    } = &cond_expr.kind
                    {
                        if *operator == "<" {
                            if let (
                                ExpressionKind::Identifier(name),
                                ExpressionKind::Literal(Literal::Number(limit)),
                            ) = (&left.kind, &right.kind)
                            {
                                let const_idx = self.chunk.add_constant(Constant::Number(*limit));

                                if let Some(slot) = self.resolve_local(name) {
                                    // Local optimized candidate
                                    self.compile_expression(cond_expr)?;
                                    let jump_idx = self.chunk.code.len();
                                    self.chunk
                                        .emit1(op::JUMP_IF_FALSE, 0, cond_expr.line as u32);
                                    self.chunk.emit0(op::POP, cond_expr.line as u32);
                                    exit_jump = Some(jump_idx);
                                    optimized_condition =
                                        Some((true, slot as u32, const_idx as u32)); // true = is_local
                                    handled = true;
                                } else if !self.is_class {
                                    // Global optimized candidate
                                    let name_lower = name.to_lowercase();
                                    let name_idx = self.chunk.add_constant(Constant::String(
                                        BoxString::new(name_lower.as_str()),
                                    ));

                                    // Initial check (standard instructions)
                                    self.compile_expression(cond_expr)?;
                                    let jump_idx = self.chunk.code.len();
                                    self.chunk
                                        .emit1(op::JUMP_IF_FALSE, 0, cond_expr.line as u32);
                                    self.chunk.emit0(op::POP, cond_expr.line as u32);
                                    exit_jump = Some(jump_idx);
                                    optimized_condition =
                                        Some((false, name_idx as u32, const_idx as u32)); // false = is_local
                                    handled = true;
                                }
                            }
                        }
                    }

                    if !handled {
                        self.compile_expression(cond_expr)?;
                        let jump_idx = self.chunk.code.len();
                        self.chunk.emit1(op::JUMP_IF_FALSE, 0, stmt.line as u32);
                        self.chunk.emit0(op::POP, stmt.line as u32);
                        exit_jump = Some(jump_idx);
                    }
                }

                let body_start = self.chunk.code.len();

                self.continue_patches.push(Vec::new());
                self.break_patches.push(Vec::new());
                self.loop_locals.push(self.locals.len());
                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }
                let loop_local_count = self.loop_locals.last().copied().unwrap_or(0);
                self.loop_locals.pop();

                // Pop any locals declared inside the loop body before update/loop.
                while self.locals.len() > loop_local_count {
                    self.chunk.emit0(op::POP, stmt.line as u32);
                    self.locals.pop();
                }

                // Patch all continue jumps to land here (before the update step)
                let continue_target = self.chunk.code.len();
                if let Some(patches) = self.continue_patches.pop() {
                    for idx in patches {
                        let offset = continue_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                let mut merged_into_step = false;
                if let Some(update_expr) = update {
                    let mut optimized_update = false;
                    if let ExpressionKind::Postfix { base, operator } = &update_expr.kind {
                        if *operator == "++" {
                            if let ExpressionKind::Identifier(name) = &base.kind {
                                if let Some(slot) = self.resolve_local(name) {
                                    // If condition is also `this_local < CONST`, merge into FOR_LOOP_STEP
                                    let can_merge = if let Some((is_local_cond, cond_slot, _)) =
                                        optimized_condition
                                    {
                                        is_local_cond && cond_slot == slot as u32
                                    } else {
                                        false
                                    };
                                    if can_merge {
                                        merged_into_step = true;
                                        // INC will be handled by FOR_LOOP_STEP; don't emit INC_LOCAL
                                    } else {
                                        self.chunk.emit1(
                                            op::INC_LOCAL,
                                            slot as u32,
                                            update_expr.line,
                                        );
                                    }
                                    optimized_update = true;
                                } else if !self.is_class {
                                    let name_idx = self.chunk.add_constant(Constant::String(
                                        BoxString::new(name.as_str()),
                                    ));
                                    self.chunk.emit1(
                                        op::INC_GLOBAL,
                                        name_idx as u32,
                                        update_expr.line,
                                    );
                                    optimized_update = true;
                                }
                            }
                        }
                    }

                    if !optimized_update {
                        self.compile_expression(update_expr)?;
                        self.chunk.emit0(op::POP, stmt.line as u32);
                    }
                }

                // When FOR_LOOP_STEP falls through (limit reached) there is no boolean
                // on the stack — the bool from the initial JUMP_IF_FALSE guard was already
                // consumed by the POP inside the loop header.  We track a JUMP emitted
                // immediately after FOR_LOOP_STEP so we can skip the bool-POP that only
                // applies to the early JUMP_IF_FALSE exit path.
                let mut for_loop_step_done_jump: Option<usize> = None;

                if let Some((is_local, idx, const_idx)) = optimized_condition {
                    if is_local && merged_into_step {
                        // Single opcode: increment local + compare + jump back
                        let offset = self.chunk.code.len() - body_start + 3;
                        self.chunk.emit3(
                            op::FOR_LOOP_STEP,
                            idx as u32,
                            const_idx as u32,
                            offset as u32,
                            stmt.line as u32,
                        );
                        // FOR_LOOP_STEP falls through here. Skip the bool-POP below.
                        let jump_ip = self.chunk.code.len();
                        self.chunk.emit1(op::JUMP, 0, stmt.line as u32);
                        for_loop_step_done_jump = Some(jump_ip);
                    } else if is_local {
                        let offset = self.chunk.code.len() - body_start + 3;
                        self.chunk.emit3(
                            op::LOCAL_COMPARE_JUMP,
                            idx as u32,
                            const_idx as u32,
                            offset as u32,
                            stmt.line as u32,
                        );
                    } else {
                        let offset = self.chunk.code.len() - body_start + 3;
                        self.chunk.emit3(
                            op::GLOBAL_COMPARE_JUMP,
                            idx as u32,
                            const_idx as u32,
                            offset as u32,
                            stmt.line as u32,
                        );
                    }
                } else {
                    let loop_end = self.chunk.code.len();
                    let offset = loop_end - loop_start + 1;
                    self.chunk.emit1(op::LOOP, offset as u32, stmt.line as u32);
                }

                // Patch JUMP_IF_FALSE to land here; emit POP to discard the bool.
                // This POP is only reached via the early JUMP_IF_FALSE exit — the
                // FOR_LOOP_STEP fallthrough jumps past it (see for_loop_step_done_jump).
                if let Some(idx) = exit_jump {
                    let exit_target = self.chunk.code.len();
                    let offset = exit_target - idx - 1;
                    self.chunk.code[idx] = op::JUMP_IF_FALSE as u32 | ((offset as u32) << 8);
                    self.chunk.emit0(op::POP, stmt.line as u32);
                }

                // Patch the FOR_LOOP_STEP done-jump to land here (after the bool-POP).
                if let Some(jump_ip) = for_loop_step_done_jump {
                    let target = self.chunk.code.len();
                    let offset = target - jump_ip - 1;
                    self.chunk.code[jump_ip] = op::JUMP as u32 | ((offset as u32) << 8);
                }

                self.end_scope();

                // Patch any break jumps to land at the end of the loop
                let break_target = self.chunk.code.len();
                if let Some(breaks) = self.break_patches.pop() {
                    for idx in breaks {
                        let offset = break_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                Ok(())
            }
            StatementKind::WhileLoop { condition, body } => {
                let loop_start = self.chunk.code.len();

                // 1. Evaluate condition
                self.compile_expression(condition)?;

                // 2. Jump to end if false
                let exit_jump = self.chunk.code.len();
                self.chunk.emit1(op::JUMP_IF_FALSE, 0, condition.line);

                // 3. Pop truthy result and execute body
                self.chunk.emit0(op::POP, condition.line);

                self.begin_scope();
                self.continue_patches.push(Vec::new());
                self.break_patches.push(Vec::new());
                self.loop_locals.push(self.locals.len());
                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }
                self.loop_locals.pop();
                self.end_scope();

                // 4. Continue target: where 'continue' jumps to (just before LOOP back)
                let continue_target = self.chunk.code.len();
                let continues = self.continue_patches.pop().unwrap();
                for idx in continues {
                    let offset = continue_target - idx - 1;
                    self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                }

                // 5. Loop back to evaluate condition again
                let loop_end = self.chunk.code.len();
                let offset = loop_end - loop_start + 1;
                self.chunk.emit1(op::LOOP, offset as u32, condition.line);

                // 6. Exit target: patch exit_jump
                let exit_target = self.chunk.code.len();
                let exit_offset = exit_target - exit_jump - 1;
                self.chunk.code[exit_jump] = op::JUMP_IF_FALSE as u32 | ((exit_offset as u32) << 8);

                // 7. Pop falsy result
                self.chunk.emit0(op::POP, condition.line);

                // Patch any break jumps to land at the end of the loop
                let break_target = self.chunk.code.len();
                if let Some(breaks) = self.break_patches.pop() {
                    for idx in breaks {
                        let offset = break_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                Ok(())
            }
            StatementKind::Switch {
                value,
                cases,
                default_case,
            } => {
                // 1. Evaluate switch value and leave it on the stack
                self.compile_expression(value)?;

                self.begin_scope();
                // Not pushing continue_patches since 'continue' isn't for switch, only 'break'.
                self.break_patches.push(Vec::new());
                self.loop_locals.push(self.locals.len());

                let mut jump_to_bodies = Vec::new();

                // 2. Generate case conditions
                for switch_case in cases {
                    self.chunk.emit0(op::DUP, stmt.line as u32);
                    self.compile_expression(&switch_case.value)?;
                    self.chunk.emit0(op::EQUAL, stmt.line as u32);

                    // If equal, jump to the body (we will patch this later)
                    let jump_idx = self.chunk.code.len();
                    self.chunk.emit1(op::JUMP_IF_FALSE, 0, stmt.line as u32);

                    // Equal: pop the comparison result
                    self.chunk.emit0(op::POP, stmt.line as u32);
                    // Also pop the switch value since we found a match
                    self.chunk.emit0(op::POP, stmt.line as u32);

                    // Jump to body start
                    let body_jump_idx = self.chunk.code.len();
                    self.chunk.emit1(op::JUMP, 0, stmt.line as u32);
                    jump_to_bodies.push(body_jump_idx);

                    // Not equal: patch the JUMP_IF_FALSE to here
                    let next_target = self.chunk.code.len();
                    let offset = next_target - jump_idx - 1;
                    self.chunk.code[jump_idx] = op::JUMP_IF_FALSE as u32 | ((offset as u32) << 8);

                    // Pop the falsy comparison result, leaving switch value on stack
                    self.chunk.emit0(op::POP, stmt.line as u32);
                }

                // 3. Generate default case jump
                let jump_to_default = self.chunk.code.len();
                // Pop the switch value as we either match default or fall out
                self.chunk.emit0(op::POP, stmt.line as u32);
                self.chunk.emit1(op::JUMP, 0, stmt.line as u32);

                // 4. Generate case bodies
                let mut i = 0;
                for switch_case in cases {
                    // Patch the conditional jump to land at this body
                    let body_start = self.chunk.code.len();
                    let offset = body_start - jump_to_bodies[i] - 1;
                    self.chunk.code[jump_to_bodies[i]] = op::JUMP as u32 | ((offset as u32) << 8);

                    for body_stmt in &switch_case.body {
                        self.compile_statement(body_stmt, false)?;
                    }
                    i += 1;
                }

                // 5. Generate default body
                let default_start = self.chunk.code.len();
                let def_offset = default_start - jump_to_default - 1;
                self.chunk.code[jump_to_default] = op::JUMP as u32 | ((def_offset as u32) << 8);

                if let Some(def_case) = default_case {
                    for body_stmt in def_case {
                        self.compile_statement(body_stmt, false)?;
                    }
                }

                // Cleanup scope and loop_locals tracking
                self.loop_locals.pop();
                self.end_scope();

                // 6. Patch breaks
                let end_target = self.chunk.code.len();
                if let Some(breaks) = self.break_patches.pop() {
                    for idx in breaks {
                        let offset = end_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                Ok(())
            }
            StatementKind::FunctionDecl {
                name,
                attributes: _,
                access_modifier: _,
                return_type: _,
                params,
                body,
            } => {
                let func = self.compile_function(&name, &params, &body)?;
                let func_idx = self.chunk.add_constant(Constant::CompiledFunction(func));
                if self.is_repl && is_last {
                    self.chunk.emit1(op::CONSTANT, func_idx, stmt.line as u32);
                }
                self.chunk.emit1(op::CONSTANT, func_idx, stmt.line as u32);
                let name_idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(name.as_str())));
                self.chunk
                    .emit1(op::DEFINE_GLOBAL, name_idx as u32, stmt.line as u32);
                Ok(())
            }
            StatementKind::ForLoop {
                item,
                index,
                collection,
                body,
            } => {
                self.begin_scope();

                self.compile_expression(collection)?;
                let collection_slot = self.locals.len();
                self.locals.push(Local {
                    name: "$collection".to_string(),
                    depth: self.scope_depth,
                });

                let zero_idx = self.chunk.add_constant(Constant::Number(0.0));
                self.chunk.emit1(op::CONSTANT, zero_idx, stmt.line as u32);
                let cursor_slot = self.locals.len();
                self.locals.push(Local {
                    name: "$cursor".to_string(),
                    depth: self.scope_depth,
                });

                let loop_start = self.chunk.code.len();

                let has_index = index.is_some();
                let iter_next_idx = self.chunk.code.len();
                self.chunk.emit_iter_next(
                    collection_slot as u32,
                    cursor_slot as u32,
                    has_index,
                    stmt.line as u32,
                );

                self.add_local(item.clone());
                if let Some(index_name) = index {
                    self.add_local(index_name.clone());
                }

                self.continue_patches.push(Vec::new());
                self.break_patches.push(Vec::new());
                self.loop_locals.push(self.locals.len());
                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }
                let loop_local_count = self.loop_locals.last().copied().unwrap_or(0);
                self.loop_locals.pop();

                // Pop any locals declared inside the loop body before looping back.
                while self.locals.len() > loop_local_count {
                    self.chunk.emit0(op::POP, stmt.line as u32);
                    self.locals.pop();
                }

                // Patch continue jumps to land here (before loop-var pops)
                let continue_target = self.chunk.code.len();
                if let Some(patches) = self.continue_patches.pop() {
                    for idx in patches {
                        let offset = continue_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                if index.is_some() {
                    self.chunk.emit0(op::POP, stmt.line as u32);
                    self.locals.pop();
                }
                self.chunk.emit0(op::POP, stmt.line as u32);
                self.locals.pop();

                let loop_end = self.chunk.code.len();
                let offset = loop_end - loop_start + 1;
                self.chunk.emit1(op::LOOP, offset as u32, stmt.line as u32);

                let exit_target = self.chunk.code.len();
                let offset = exit_target - iter_next_idx - 3;
                self.chunk.code[iter_next_idx + 2] = offset as u32;

                // Patch any break jumps to land at the end of the loop
                let break_target = self.chunk.code.len();
                if let Some(breaks) = self.break_patches.pop() {
                    for idx in breaks {
                        let offset = break_target - idx - 1;
                        self.chunk.code[idx] = op::JUMP as u32 | ((offset as u32) << 8);
                    }
                }

                self.end_scope();
                Ok(())
            }
        }
    }

    fn compile_expression(&mut self, expr: &Expression) -> Result<()> {
        self.current_line = expr.line;
        match &expr.kind {
            ExpressionKind::New { class_path, args } => {
                let lower_path = class_path.to_lowercase();
                let mut resolved_path = self
                    .imports
                    .get(&lower_path)
                    .cloned()
                    .unwrap_or(class_path.clone());

                // js: imports are bound to globals at load time; they are not instantiate-able classes.
                if resolved_path.to_lowercase().starts_with("js:") {
                    resolved_path = class_path.clone();
                }

                if resolved_path.to_lowercase().starts_with("java:") {
                    let java_class = &resolved_path[5..];

                    // Push createObject onto stack
                    let bif_name_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("createobject")));
                    self.chunk.emit1(op::GET_GLOBAL, bif_name_idx, expr.line);

                    // Push type "java"
                    let type_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("java")));
                    self.chunk.emit1(op::CONSTANT, type_idx, expr.line);

                    // Push class name
                    let class_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(java_class)));
                    self.chunk.emit1(op::CONSTANT, class_idx, expr.line);

                    // Push arguments
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }

                    // Call createObject(type, class, ...args)
                    self.chunk.emit1(op::CALL, 2 + args.len() as u32, expr.line);
                    return Ok(());
                }

                if resolved_path.to_lowercase().starts_with("rust:") {
                    let rust_class = &resolved_path[5..];

                    // Push createObject onto stack
                    let bif_name_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("createobject")));
                    self.chunk.emit1(op::GET_GLOBAL, bif_name_idx, expr.line);

                    // Push type "rust"
                    let type_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("rust")));
                    self.chunk.emit1(op::CONSTANT, type_idx, expr.line);

                    // Push class name
                    let class_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(rust_class)));
                    self.chunk.emit1(op::CONSTANT, class_idx, expr.line);

                    // Push arguments
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }

                    // Call createObject(type, class, ...args)
                    self.chunk.emit1(op::CALL, 2 + args.len() as u32, expr.line);
                    return Ok(());
                }

                let mut class_has_init = false;
                if resolved_path.contains('.') {
                    let class_val = self.load_class_from_path(&resolved_path)?;
                    if let Constant::Class(ref cls) = class_val {
                        class_has_init = cls
                            .methods
                            .iter()
                            .any(|(n, _)| n.eq_ignore_ascii_case("init"));
                    }
                    let class_idx = self.chunk.add_constant(class_val);
                    self.chunk.emit1(op::CONSTANT, class_idx, expr.line);
                } else {
                    // Check inline classes in this chunk or any parent chunk tracked
                    // via the shared class_has_init_map.
                    class_has_init = self
                        .class_has_init_map
                        .get(&resolved_path.to_lowercase())
                        .copied()
                        .unwrap_or(false);
                    if !class_has_init {
                        class_has_init = self.chunk.constants.iter().any(|c| {
                            if let Constant::Class(cls) = c {
                                cls.name.eq_ignore_ascii_case(&resolved_path)
                                    && cls
                                        .methods
                                        .iter()
                                        .any(|(n, _)| n.eq_ignore_ascii_case("init"))
                            } else {
                                false
                            }
                        });
                    }
                    let class_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(resolved_path.as_str())));
                    self.chunk.emit1(op::GET_GLOBAL, class_idx, expr.line);
                }

                let mut arg_names = Vec::new();
                let mut has_named = false;
                for arg in args {
                    if let Some(name) = &arg.name {
                        arg_names.push(name.clone());
                        has_named = true;
                    } else {
                        arg_names.push("".to_string());
                    }
                    self.compile_expression(&arg.value)?;
                }

                // OpNew(args.len()) replaces Class with Instance BELOW args
                // and sets up the constructor frame
                self.chunk.emit1(op::NEW, args.len() as u32, expr.line);

                // Call init() if args were passed or if the class is known to define init().
                if !args.is_empty() || class_has_init {
                    let name_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("init")));
                    if has_named {
                        let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                        self.chunk.emit3(
                            op::INVOKE_NAMED,
                            name_idx as u32,
                            args.len() as u32,
                            names_idx as u32,
                            expr.line,
                        );
                    } else {
                        self.chunk
                            .emit2(op::INVOKE, name_idx as u32, args.len() as u32, expr.line);
                    }
                }

                Ok(())
            }
            ExpressionKind::Literal(lit) => match lit {
                Literal::Number(n) => {
                    let idx = self.chunk.add_constant(Constant::Number(*n));
                    self.chunk.emit1(op::CONSTANT, idx as u32, expr.line);
                    Ok(())
                }
                Literal::String(parts) => {
                    if parts.is_empty() {
                        let idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new("")));
                        self.chunk.emit1(op::CONSTANT, idx as u32, expr.line);
                        return Ok(());
                    }
                    self.compile_string_part(&parts[0])?;
                    for i in 1..parts.len() {
                        self.compile_string_part(&parts[i])?;
                        self.chunk.emit0(op::STRING_CONCAT, expr.line);
                    }
                    Ok(())
                }
                Literal::Boolean(b) => {
                    let idx = self.chunk.add_constant(Constant::Boolean(*b));
                    self.chunk.emit1(op::CONSTANT, idx as u32, expr.line);
                    Ok(())
                }
                Literal::Null => {
                    let idx = self.chunk.add_constant(Constant::Null);
                    self.chunk.emit1(op::CONSTANT, idx as u32, expr.line);
                    Ok(())
                }
                Literal::Array(items) => {
                    for item in items {
                        self.compile_expression(item)?;
                    }
                    self.chunk.emit1(op::ARRAY, items.len() as u32, expr.line);
                    Ok(())
                }
                Literal::Struct(members) => {
                    for (key_expr, val_expr) in members {
                        match &key_expr.kind {
                            ExpressionKind::Identifier(name) => {
                                let idx = self
                                    .chunk
                                    .add_constant(Constant::String(BoxString::new(name.as_str())));
                                self.chunk.emit1(op::CONSTANT, idx as u32, expr.line);
                            }
                            _ => self.compile_expression(key_expr)?,
                        }
                        self.compile_expression(val_expr)?;
                    }
                    self.chunk
                        .emit1(op::STRUCT, members.len() as u32, expr.line);
                    Ok(())
                }
                Literal::Function { params, body } => {
                    let anon_name = format!("anonymous@{}@{}", expr.line, self.chunk.code.len());
                    let func = self.compile_function(&anon_name, &params, &body)?;
                    let func_idx = self.chunk.add_constant(Constant::CompiledFunction(func));
                    self.chunk.emit1(op::CONSTANT, func_idx, expr.line);
                    Ok(())
                }
            },
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                // Short-circuit operators handle their own right-side compilation
                match operator.as_str() {
                    "||" => {
                        self.compile_expression(left)?;
                        // If left is falsy, skip the unconditional jump to land at pop+right
                        let jif_idx = self.chunk.code.len();
                        self.chunk.emit1(op::JUMP_IF_FALSE, 0, expr.line);
                        // Left is truthy: jump past pop+right to end
                        let jump_idx = self.chunk.code.len();
                        self.chunk.emit1(op::JUMP, 0, expr.line);
                        // False path: pop falsy left, evaluate right
                        let false_target = self.chunk.code.len();
                        self.chunk.code[jif_idx] =
                            op::JUMP_IF_FALSE as u32 | (((false_target - jif_idx - 1) as u32) << 8);
                        self.chunk.emit0(op::POP, expr.line);
                        self.compile_expression(right)?;
                        // Patch unconditional jump to end
                        let end_target = self.chunk.code.len();
                        self.chunk.code[jump_idx] =
                            op::JUMP as u32 | (((end_target - jump_idx - 1) as u32) << 8);
                        return Ok(());
                    }
                    "&&" => {
                        self.compile_expression(left)?;
                        // If left is falsy, jump to end (left stays on stack as result)
                        let jif_idx = self.chunk.code.len();
                        self.chunk.emit1(op::JUMP_IF_FALSE, 0, expr.line);
                        // Left is truthy: pop it, evaluate right
                        self.chunk.emit0(op::POP, expr.line);
                        self.compile_expression(right)?;
                        // Patch JumpIfFalse to end
                        let end_target = self.chunk.code.len();
                        self.chunk.code[jif_idx] =
                            op::JUMP_IF_FALSE as u32 | (((end_target - jif_idx - 1) as u32) << 8);
                        return Ok(());
                    }
                    _ => {}
                }
                // All other operators: eager evaluation
                self.compile_expression(left)?;
                self.compile_expression(right)?;
                match operator.as_str() {
                    "+" => {
                        let mut specialized = false;
                        if let (
                            ExpressionKind::Literal(Literal::Number(a)),
                            ExpressionKind::Literal(Literal::Number(b)),
                        ) = (&left.kind, &right.kind)
                        {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.emit0(op::ADD_INT, expr.line);
                            } else {
                                self.chunk.emit0(op::ADD_FLOAT, expr.line);
                            }
                            specialized = true;
                        }

                        if !specialized {
                            self.chunk.emit0(op::ADD, expr.line);
                        }
                    }
                    "-" => {
                        let mut specialized = false;
                        if let (
                            ExpressionKind::Literal(Literal::Number(a)),
                            ExpressionKind::Literal(Literal::Number(b)),
                        ) = (&left.kind, &right.kind)
                        {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.emit0(op::SUB_INT, expr.line);
                            } else {
                                self.chunk.emit0(op::SUB_FLOAT, expr.line);
                            }
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.emit0(op::SUBTRACT, expr.line);
                        }
                    }
                    "*" => {
                        let mut specialized = false;
                        if let (
                            ExpressionKind::Literal(Literal::Number(a)),
                            ExpressionKind::Literal(Literal::Number(b)),
                        ) = (&left.kind, &right.kind)
                        {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.emit0(op::MUL_INT, expr.line);
                            } else {
                                self.chunk.emit0(op::MUL_FLOAT, expr.line);
                            }
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.emit0(op::MULTIPLY, expr.line);
                        }
                    }
                    "/" => {
                        let mut specialized = false;
                        if let (
                            ExpressionKind::Literal(Literal::Number(_)),
                            ExpressionKind::Literal(Literal::Number(_)),
                        ) = (&left.kind, &right.kind)
                        {
                            self.chunk.emit0(op::DIV_FLOAT, expr.line);
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.emit0(op::DIVIDE, expr.line);
                        }
                    }
                    "%" => self.chunk.emit0(op::MODULO, expr.line),
                    "&" => self.chunk.emit0(op::STRING_CONCAT, expr.line),
                    "==" => self.chunk.emit0(op::EQUAL, expr.line),
                    "!=" => self.chunk.emit0(op::NOT_EQUAL, expr.line),
                    "<" => self.chunk.emit0(op::LESS, expr.line),
                    "<=" => self.chunk.emit0(op::LESS_EQUAL, expr.line),
                    ">" => self.chunk.emit0(op::GREATER, expr.line),
                    ">=" => self.chunk.emit0(op::GREATER_EQUAL, expr.line),
                    _ => bail!("Unknown operator: {}", operator),
                }
                Ok(())
            }
            ExpressionKind::UnaryNot(inner) => {
                self.compile_expression(inner)?;
                self.chunk.emit0(op::NOT, expr.line);
                Ok(())
            }
            ExpressionKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                // Compile condition
                self.compile_expression(condition)?;
                // Jump to else branch if false
                let jif_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP_IF_FALSE, 0, expr.line);
                // True branch
                self.chunk.emit0(op::POP, expr.line);
                self.compile_expression(then_expr)?;
                // Jump over else branch
                let jump_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, expr.line);
                // False branch
                let else_target = self.chunk.code.len();
                self.chunk.code[jif_idx] =
                    op::JUMP_IF_FALSE as u32 | (((else_target - jif_idx - 1) as u32) << 8);
                self.chunk.emit0(op::POP, expr.line);
                self.compile_expression(else_expr)?;
                // Patch end jump
                let end_target = self.chunk.code.len();
                self.chunk.code[jump_idx] =
                    op::JUMP as u32 | (((end_target - jump_idx - 1) as u32) << 8);
                Ok(())
            }
            ExpressionKind::Elvis { left, right } => {
                self.compile_expression(left)?;
                // If null, skip unconditional jump to land at pop+right
                let jmp_null_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP_IF_NULL, 0, expr.line);
                // If not null, jump over the right expression
                let jmp_end_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP, 0, expr.line);
                // Null branch
                let null_target = self.chunk.code.len();
                self.chunk.code[jmp_null_idx] =
                    op::JUMP_IF_NULL as u32 | (((null_target - jmp_null_idx - 1) as u32) << 8);
                self.chunk.emit0(op::POP, expr.line); // Pop the null
                self.compile_expression(right)?;
                // Patch end jump
                let end_target = self.chunk.code.len();
                self.chunk.code[jmp_end_idx] =
                    op::JUMP as u32 | (((end_target - jmp_end_idx - 1) as u32) << 8);
                Ok(())
            }
            ExpressionKind::Identifier(name) => {
                let lower_name = name.to_lowercase();
                let is_js_import = self
                    .imports
                    .get(&lower_name)
                    .map(|p| p.to_lowercase().starts_with("js:"))
                    == Some(true);
                if lower_name == "this" {
                    let idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("this")));
                    self.chunk.emit1(op::GET_PRIVATE, idx as u32, expr.line);
                } else if lower_name == "variables" {
                    let idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new("variables")));
                    self.chunk.emit1(op::GET_PRIVATE, idx as u32, expr.line);
                } else if let Some(slot) = self.resolve_local(&name) {
                    self.chunk.emit1(op::GET_LOCAL, slot as u32, expr.line);
                } else if self.is_class && !is_js_import {
                    let idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(name.as_str())));
                    self.chunk.emit1(op::GET_PRIVATE, idx as u32, expr.line);
                } else {
                    let idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(name.as_str())));
                    self.chunk.emit1(op::GET_GLOBAL, idx as u32, expr.line);
                }
                Ok(())
            }
            ExpressionKind::Assignment { target, value } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        let lower_name = name.to_lowercase();
                        if lower_name == "variables" {
                            bail!("Cannot assign to the 'variables' scope directly");
                        }
                        let is_js_import = self
                            .imports
                            .get(&lower_name)
                            .map(|p| p.to_lowercase().starts_with("js:"))
                            == Some(true);
                        self.compile_expression(value)?;
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.emit1(op::SET_LOCAL, slot as u32, expr.line);
                        } else if self.is_class && !is_js_import {
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk
                                .emit1(op::SET_PRIVATE, name_idx as u32, expr.line);
                        } else {
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk.emit1(op::SET_GLOBAL, name_idx as u32, expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.compile_expression(value)?;
                        let name_idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new(member.as_str())));
                        self.chunk.emit1(op::SET_MEMBER, name_idx as u32, expr.line);
                    }
                    crate::ast::AssignmentTarget::Index { base, index } => {
                        self.compile_expression(base)?;
                        self.compile_expression(index)?;
                        self.compile_expression(value)?;
                        self.chunk.emit0(op::SET_INDEX, expr.line);
                    }
                }
                Ok(())
            }
            ExpressionKind::FunctionCall { base, args } => {
                let mut arg_names = Vec::new();
                let mut has_named = false;

                for arg in args {
                    if let Some(name) = &arg.name {
                        arg_names.push(name.to_lowercase());
                        has_named = true;
                    } else {
                        arg_names.push("".to_string());
                    }
                }

                if let ExpressionKind::Identifier(name) = &base.kind {
                    let lower_name = name.to_lowercase();
                    if lower_name == "println" || lower_name == "echo" {
                        for arg in args {
                            self.compile_expression(&arg.value)?;
                        }
                        self.chunk.emit1(op::PRINTLN, args.len() as u32, expr.line);
                        let null_idx = self.chunk.add_constant(Constant::Null);
                        self.chunk.emit1(op::CONSTANT, null_idx, expr.line);
                        return Ok(());
                    }
                    if lower_name == "print" {
                        for arg in args {
                            self.compile_expression(&arg.value)?;
                        }
                        self.chunk.emit1(op::PRINT, args.len() as u32, expr.line);
                        let null_idx = self.chunk.add_constant(Constant::Null);
                        self.chunk.emit1(op::CONSTANT, null_idx, expr.line);
                        return Ok(());
                    }
                }

                // BoxLang compatibility: unqualified method calls inside a class
                // implicitly use `this` as the receiver.
                if self.is_class {
                    if let ExpressionKind::Identifier(name) = &base.kind {
                        if self.class_methods.contains(&name.to_lowercase()) {
                            let this_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new("this")));
                            self.chunk
                                .emit1(op::GET_PRIVATE, this_idx as u32, expr.line);
                            for arg in args {
                                self.compile_expression(&arg.value)?;
                            }
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            if has_named {
                                let names_idx =
                                    self.chunk.add_constant(Constant::StringArray(arg_names));
                                self.chunk.emit3(
                                    op::INVOKE_NAMED,
                                    name_idx as u32,
                                    args.len() as u32,
                                    names_idx as u32,
                                    expr.line,
                                );
                            } else {
                                self.chunk.emit2(
                                    op::INVOKE,
                                    name_idx as u32,
                                    args.len() as u32,
                                    expr.line,
                                );
                            }
                            return Ok(());
                        }
                    }
                }

                if let ExpressionKind::MemberAccess {
                    base: member_base,
                    member,
                } = &base.kind
                {
                    self.compile_expression(member_base)?;
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }
                    let name_idx = self
                        .chunk
                        .add_constant(Constant::String(BoxString::new(member.as_str())));
                    if has_named {
                        let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                        self.chunk.emit3(
                            op::INVOKE_NAMED,
                            name_idx as u32,
                            args.len() as u32,
                            names_idx as u32,
                            expr.line,
                        );
                    } else {
                        self.chunk
                            .emit2(op::INVOKE, name_idx as u32, args.len() as u32, expr.line);
                    }
                    return Ok(());
                }

                self.compile_expression(base)?;
                for arg in args {
                    self.compile_expression(&arg.value)?;
                }
                if has_named {
                    let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                    self.chunk.emit2(
                        op::CALL_NAMED,
                        args.len() as u32,
                        names_idx as u32,
                        expr.line,
                    );
                } else {
                    self.chunk.emit1(op::CALL, args.len() as u32, expr.line);
                }
                Ok(())
            }
            ExpressionKind::ArrayAccess { base, index } => {
                self.compile_expression(base)?;
                self.compile_expression(index)?;
                self.chunk.emit0(op::INDEX, expr.line);
                Ok(())
            }
            ExpressionKind::MemberAccess { base, member } => {
                self.compile_expression(base)?;
                let name_idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(member.as_str())));
                self.chunk.emit1(op::MEMBER, name_idx, expr.line);
                Ok(())
            }
            ExpressionKind::SafeMemberAccess { base, member } => {
                self.compile_expression(base)?;
                let jmp_null_idx = self.chunk.code.len();
                self.chunk.emit1(op::JUMP_IF_NULL, 0, expr.line); // if null, leave null on stack and jump over member access
                let name_idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(member.as_str())));
                self.chunk.emit1(op::MEMBER, name_idx, expr.line);
                let end_target = self.chunk.code.len();
                self.chunk.code[jmp_null_idx] =
                    op::JUMP_IF_NULL as u32 | (((end_target - jmp_null_idx - 1) as u32) << 8);
                Ok(())
            }
            ExpressionKind::Prefix { operator, target } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        self.compile_expression(&Expression::new(
                            ExpressionKind::Identifier(name.clone()),
                            expr.line,
                        ))?;
                        if operator == "++" {
                            self.chunk.emit0(op::INC, expr.line);
                        } else {
                            self.chunk.emit0(op::DEC, expr.line);
                        }
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.emit1(op::SET_LOCAL, slot as u32, expr.line);
                        } else if self.is_class {
                            let idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk.emit1(op::SET_PRIVATE, idx as u32, expr.line);
                        } else {
                            let idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk.emit1(op::SET_GLOBAL, idx as u32, expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.chunk.emit0(op::DUP, expr.line);
                        let name_idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new(member.as_str())));
                        self.chunk.emit1(op::MEMBER, name_idx, expr.line);
                        if operator == "++" {
                            self.chunk.emit0(op::INC, expr.line);
                        } else {
                            self.chunk.emit0(op::DEC, expr.line);
                        }
                        self.chunk.emit1(op::SET_MEMBER, name_idx as u32, expr.line);
                    }
                    crate::ast::AssignmentTarget::Index { base: _, index: _ } => {
                        bail!("Prefix ops on indexed targets not yet implemented");
                    }
                }
                Ok(())
            }
            ExpressionKind::Postfix { base, operator } => {
                match &base.kind {
                    ExpressionKind::Identifier(name) => {
                        self.compile_expression(base)?;
                        self.chunk.emit0(op::DUP, expr.line);
                        if operator == "++" {
                            self.chunk.emit0(op::INC, expr.line);
                        } else {
                            self.chunk.emit0(op::DEC, expr.line);
                        }
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.emit1(op::SET_LOCAL, slot as u32, expr.line);
                        } else if self.is_class {
                            let idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk.emit1(op::SET_PRIVATE, idx as u32, expr.line);
                        } else {
                            let idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            self.chunk.emit1(op::SET_GLOBAL, idx as u32, expr.line);
                        }
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                    ExpressionKind::MemberAccess {
                        base: member_base,
                        member,
                    } => {
                        self.compile_expression(member_base)?;
                        self.chunk.emit0(op::DUP, expr.line);
                        let name_idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new(member.as_str())));
                        self.chunk.emit1(op::MEMBER, name_idx, expr.line);
                        self.chunk.emit0(op::SWAP, expr.line);
                        self.chunk.emit0(op::OVER, expr.line);
                        if operator == "++" {
                            self.chunk.emit0(op::INC, expr.line);
                        } else {
                            self.chunk.emit0(op::DEC, expr.line);
                        }
                        self.chunk.emit1(op::SET_MEMBER, name_idx as u32, expr.line);
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                    _ => bail!("Postfix ops only supported on identifiers and members currently"),
                }
                Ok(())
            }
        }
    }

    fn compile_string_part(&mut self, part: &StringPart) -> Result<()> {
        match part {
            StringPart::Text(t) => {
                let idx = self
                    .chunk
                    .add_constant(Constant::String(BoxString::new(t.as_str())));
                self.chunk
                    .emit1(op::CONSTANT, idx as u32, self.current_line);
                Ok(())
            }
            StringPart::Expression(expr) => self.compile_expression(expr),
        }
    }

    fn compile_function(
        &mut self,
        name: &str,
        params: &[crate::ast::FunctionParam],
        body: &crate::ast::FunctionBody,
    ) -> Result<BxCompiledFunction> {
        let mut sub_compiler = Compiler::with_chunk(self.chunk.new_sub_chunk());
        // Source text lives only in the root chunk to avoid N copies per file.
        // The VM falls back to disk when chunk.source is empty.
        sub_compiler.scope_depth = 1;
        sub_compiler.is_class = self.is_class;
        sub_compiler.imports = self.imports.clone();
        sub_compiler.module_paths = self.module_paths.clone();
        sub_compiler.source_dir = self.source_dir.clone();
        sub_compiler.class_methods = self.class_methods.clone();
        sub_compiler.current_line = self.current_line;
        sub_compiler.class_has_init_map = self.class_has_init_map.clone();

        let mut min_arity = 0;
        for (i, param) in params.iter().enumerate() {
            if param.required {
                min_arity = (i + 1) as u32;
            }
            sub_compiler.locals.push(Local {
                name: param.name.to_lowercase(),
                depth: 1,
            });
        }

        // Emit default value logic at the beginning of the function
        for (i, param) in params.iter().enumerate() {
            if let Some(default_expr) = &param.default_value {
                sub_compiler
                    .chunk
                    .emit1(op::GET_LOCAL, i as u32, self.current_line);
                let null_idx = sub_compiler.chunk.add_constant(Constant::Null);
                sub_compiler
                    .chunk
                    .emit1(op::CONSTANT, null_idx, self.current_line);
                sub_compiler.chunk.emit0(op::EQUAL, self.current_line);

                let jump_idx = sub_compiler.chunk.code.len();
                sub_compiler
                    .chunk
                    .emit1(op::JUMP_IF_FALSE, 0, self.current_line);

                // True branch: value IS null
                sub_compiler.chunk.emit0(op::POP, self.current_line); // pop true
                sub_compiler.compile_expression(default_expr)?;
                sub_compiler
                    .chunk
                    .emit1(op::SET_LOCAL, i as u32, self.current_line);
                sub_compiler.chunk.emit0(op::POP, self.current_line); // pop set value

                let end_jump_idx = sub_compiler.chunk.code.len();
                sub_compiler.chunk.emit1(op::JUMP, 0, self.current_line);

                // False target: value IS NOT null
                let false_target = sub_compiler.chunk.code.len();
                let offset = false_target - jump_idx - 1;
                sub_compiler.chunk.code[jump_idx] =
                    op::JUMP_IF_FALSE as u32 | ((offset as u32) << 8);
                sub_compiler.chunk.emit0(op::POP, self.current_line); // pop false

                let end_target = sub_compiler.chunk.code.len();
                let end_offset = end_target - end_jump_idx - 1;
                sub_compiler.chunk.code[end_jump_idx] =
                    op::JUMP as u32 | ((end_offset as u32) << 8);
            }
        }

        // BoxLang compatibility: create `arguments` scope struct.
        sub_compiler.locals.push(Local {
            name: "arguments".to_string(),
            depth: 1,
        });
        let args_local_idx = params.len() as u32;
        for (i, param) in params.iter().enumerate() {
            let key_idx = sub_compiler
                .chunk
                .add_constant(Constant::String(BoxString::new(&param.name)));
            sub_compiler
                .chunk
                .emit1(op::CONSTANT, key_idx, self.current_line);
            sub_compiler
                .chunk
                .emit1(op::GET_LOCAL, i as u32, self.current_line);
        }
        sub_compiler
            .chunk
            .emit1(op::STRUCT, params.len() as u32, self.current_line);
        sub_compiler
            .chunk
            .emit1(op::SET_LOCAL, args_local_idx, self.current_line);
        sub_compiler.chunk.emit0(op::POP, self.current_line);

        match body {
            FunctionBody::Block(stmts) => {
                for stmt in stmts {
                    sub_compiler.compile_statement(stmt, false)?;
                }
                let null_idx = sub_compiler.chunk.add_constant(Constant::Null);
                sub_compiler
                    .chunk
                    .emit1(op::CONSTANT, null_idx, sub_compiler.current_line);
                sub_compiler
                    .chunk
                    .emit0(op::RETURN, sub_compiler.current_line);
            }
            FunctionBody::Expression(expr) => {
                sub_compiler.compile_expression(expr)?;
                sub_compiler
                    .chunk
                    .emit0(op::RETURN, sub_compiler.current_line);
            }
            FunctionBody::Abstract => {
                bail!("Cannot compile an abstract function body");
            }
        }

        Ok(BxCompiledFunction {
            name: name.to_string(),
            arity: (params.len() + 1) as u32, // +1 for the implicit `arguments` local
            min_arity,
            params: params.iter().map(|p| p.name.to_lowercase()).collect(),
            captured_receiver: None,
            chunk: sub_compiler.chunk,
        })
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        let lower_name = name.to_lowercase();
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name == lower_name {
                return Some(i);
            }
        }
        None
    }

    fn compile_expression_as_statement(&mut self, expr: &Expression) -> Result<()> {
        match &expr.kind {
            ExpressionKind::Assignment { target, value } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        let lower_name = name.to_lowercase();
                        if lower_name == "variables" {
                            bail!("Cannot assign to the 'variables' scope directly");
                        }
                        self.compile_expression(value)?;
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.emit1(op::SET_LOCAL_POP, slot as u32, expr.line);
                        } else if self.is_class {
                            let name_idx = self.chunk.add_constant(Constant::String(
                                BoxString::new(lower_name.as_str()),
                            ));
                            self.chunk
                                .emit1(op::SET_PRIVATE, name_idx as u32, expr.line);
                            self.chunk.emit0(op::POP, expr.line as u32);
                        } else {
                            let name_idx = self.chunk.add_constant(Constant::String(
                                BoxString::new(lower_name.as_str()),
                            ));
                            self.chunk
                                .emit1(op::SET_GLOBAL_POP, name_idx as u32, expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.compile_expression(value)?;
                        let name_idx = self
                            .chunk
                            .add_constant(Constant::String(BoxString::new(member.as_str())));
                        self.chunk.emit1(op::SET_MEMBER, name_idx as u32, expr.line);
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                    crate::ast::AssignmentTarget::Index { base, index } => {
                        self.compile_expression(base)?;
                        self.compile_expression(index)?;
                        self.compile_expression(value)?;
                        self.chunk.emit0(op::SET_INDEX, expr.line);
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                }
                Ok(())
            }
            ExpressionKind::Postfix { base, operator } => {
                match &base.kind {
                    ExpressionKind::Identifier(name) => {
                        if let Some(slot) = self.resolve_local(&name) {
                            if *operator == "++" {
                                self.chunk.emit1(op::INC_LOCAL, slot as u32, expr.line);
                            } else {
                                self.compile_expression(base)?;
                                self.chunk.emit0(op::DEC, expr.line);
                                self.chunk.emit1(op::SET_LOCAL_POP, slot as u32, expr.line);
                            }
                        } else if !self.is_class {
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            if *operator == "++" {
                                self.chunk.emit1(op::INC_GLOBAL, name_idx as u32, expr.line);
                            } else {
                                self.compile_expression(base)?;
                                self.chunk.emit0(op::DEC, expr.line);
                                self.chunk
                                    .emit1(op::SET_GLOBAL_POP, name_idx as u32, expr.line);
                            }
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.emit0(op::POP, expr.line as u32);
                        }
                    }
                    ExpressionKind::MemberAccess {
                        base: member_base,
                        member,
                    } => {
                        if *operator == "++" {
                            self.compile_expression(member_base)?;
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(member.as_str())));
                            self.chunk.emit1(op::INC_MEMBER, name_idx as u32, expr.line);
                            self.chunk.emit0(op::POP, expr.line as u32); // INC_MEMBER pushes NEW value, we pop it
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.emit0(op::POP, expr.line as u32);
                        }
                    }
                    _ => {
                        self.compile_expression(expr)?;
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                }
                Ok(())
            }
            ExpressionKind::Prefix { operator, target } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        if let Some(slot) = self.resolve_local(&name) {
                            if *operator == "++" {
                                self.chunk.emit1(op::INC_LOCAL, slot as u32, expr.line);
                            } else {
                                self.compile_expression(&Expression::new(
                                    ExpressionKind::Identifier(name.clone()),
                                    expr.line,
                                ))?;
                                self.chunk.emit0(op::DEC, expr.line);
                                self.chunk.emit1(op::SET_LOCAL_POP, slot as u32, expr.line);
                            }
                        } else if !self.is_class {
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(name.as_str())));
                            if *operator == "++" {
                                self.chunk.emit1(op::INC_GLOBAL, name_idx as u32, expr.line);
                            } else {
                                self.compile_expression(&Expression::new(
                                    ExpressionKind::Identifier(name.clone()),
                                    expr.line,
                                ))?;
                                self.chunk.emit0(op::DEC, expr.line);
                                self.chunk
                                    .emit1(op::SET_GLOBAL_POP, name_idx as u32, expr.line);
                            }
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.emit0(op::POP, expr.line as u32);
                        }
                    }
                    crate::ast::AssignmentTarget::Member {
                        base: member_base,
                        member,
                    } => {
                        if *operator == "++" {
                            self.compile_expression(member_base)?;
                            let name_idx = self
                                .chunk
                                .add_constant(Constant::String(BoxString::new(member.as_str())));
                            self.chunk.emit1(op::INC_MEMBER, name_idx as u32, expr.line);
                            self.chunk.emit0(op::POP, expr.line as u32);
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.emit0(op::POP, expr.line as u32);
                        }
                    }
                    _ => {
                        self.compile_expression(expr)?;
                        self.chunk.emit0(op::POP, expr.line as u32);
                    }
                }
                Ok(())
            }
            _ => {
                self.compile_expression(expr)?;
                self.chunk.emit0(op::POP, expr.line as u32);
                Ok(())
            }
        }
    }

    fn add_local(&mut self, name: String) {
        self.locals.push(Local {
            name: name.to_lowercase(),
            depth: self.scope_depth,
        });
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self) {
        self.scope_depth -= 1;
        while let Some(local) = self.locals.last() {
            if local.depth > self.scope_depth {
                self.locals.pop();
                self.chunk.emit0(op::POP, self.current_line);
            } else {
                break;
            }
        }
    }

    fn resolve_source_file(&self, dotted_path: &str) -> Result<PathBuf> {
        let lower = dotted_path.to_lowercase();

        // Resolve bxModules.{name}.{...} paths via the module registry.
        let base_path: PathBuf = if lower.starts_with("bxmodules.") {
            let mut found = None;
            for (prefix, module_dir) in &self.module_paths {
                let prefix_with_dot = format!("{}.", prefix); // e.g. "bxmodules.foo."
                if lower.starts_with(&prefix_with_dot) {
                    let tail = &dotted_path[prefix_with_dot.len()..]; // preserve original case
                    let rel = tail.replace('.', "/");
                    found = Some(module_dir.join(rel));
                    break;
                }
            }
            found.ok_or_else(|| {
                anyhow::anyhow!(
                    "No module registered for import path '{}'. \
                     Declare it in matchbox.toml or pass --module <path>.",
                    dotted_path
                )
            })?
        } else {
            Path::new(&dotted_path.replace('.', "/")).to_path_buf()
        };

        // Try .bxs first, then fall back to .bx for BoxLang compatibility.
        let with_bxs = base_path.with_extension("bxs");
        if with_bxs.exists() {
            return Ok(with_bxs);
        }
        let with_bx = base_path.with_extension("bx");
        if with_bx.exists() {
            return Ok(with_bx);
        }

        // If the path is a simple name (single component) and source_dir is set,
        // try resolving relative to that directory (e.g. sibling interfaces).
        if base_path.components().count() == 1 {
            if let Some(ref dir) = self.source_dir {
                let rel_bxs = dir.join(&base_path).with_extension("bxs");
                if rel_bxs.exists() {
                    return Ok(rel_bxs);
                }
                let rel_bx = dir.join(&base_path).with_extension("bx");
                if rel_bx.exists() {
                    return Ok(rel_bx);
                }
            }
        }

        bail!(
            "Source file not found: {} (tried .bxs and .bx)",
            base_path.display()
        )
    }

    fn infer_class_name_from_filename(ast: &mut [Statement], filename: &str) {
        let stem = Path::new(filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(filename);
        for stmt in ast.iter_mut() {
            match &mut stmt.kind {
                StatementKind::ClassDecl { name, .. }
                | StatementKind::InterfaceDecl { name, .. } => {
                    if name.is_empty() {
                        *name = stem.to_string();
                    }
                }
                _ => {}
            }
        }
    }

    /// Emit binding bytecode for a `js:` import into the current chunk.
    /// `path` is the full import path (e.g. "js:TextEncoder"), `alias` is the
    /// desired global name (e.g. "TextEncoder").
    fn emit_js_import_binding(&mut self, path: &str, alias: &str) {
        let js_path = &path[3..];
        let segments: Vec<&str> = js_path.split('.').collect();

        let js_global = self
            .chunk
            .add_constant(Constant::String(BoxString::new("js")));
        self.chunk
            .emit1(op::GET_GLOBAL, js_global, self.current_line);

        for segment in segments {
            let idx = self
                .chunk
                .add_constant(Constant::String(BoxString::new(segment)));
            self.chunk.emit1(op::MEMBER, idx, self.current_line);
        }

        let alias_idx = self
            .chunk
            .add_constant(Constant::String(BoxString::new(alias)));
        self.chunk
            .emit1(op::SET_GLOBAL_POP, alias_idx, self.current_line);
    }

    /// Propagate any `js:` imports discovered by a sub-compiler back into
    /// this compiler so the binding bytecode is emitted in the chunk that
    /// will actually be executed by the VM.
    fn propagate_js_imports(&mut self, sub_imports: &HashMap<String, String>) {
        for (alias, path) in sub_imports {
            if path.to_lowercase().starts_with("js:") && !self.imports.contains_key(alias) {
                let dotted_path = &path[3..];
                let original_alias = dotted_path.split('.').last().unwrap();
                self.emit_js_import_binding(path, original_alias);
                self.imports.insert(alias.clone(), path.clone());
            }
        }
    }

    fn load_class_from_path(&mut self, class_path: &str) -> Result<Constant> {
        let file_path = self.resolve_source_file(class_path)?;

        let source = fs::read_to_string(&file_path)?;
        let mut ast = crate::parser::parse(&source, file_path.to_str())
            .map_err(|e| anyhow::anyhow!("Parse Error in {}: {}", class_path, e))?;

        let filename = file_path.to_str().unwrap_or(class_path);
        Self::infer_class_name_from_filename(&mut ast, filename);
        let mut sub_compiler = Compiler::with_chunk(Chunk::new(filename));
        sub_compiler.imports = self.imports.clone();
        sub_compiler.module_paths = self.module_paths.clone();
        sub_compiler.source_dir = file_path.parent().map(|p| p.to_path_buf());
        sub_compiler.is_class = true;
        sub_compiler.current_line = self.current_line;
        sub_compiler.class_has_init_map = self.class_has_init_map.clone();

        // Collect method names from the loaded class AST so unqualified calls resolve.
        for stmt in &ast {
            if let StatementKind::ClassDecl { members, .. } = &stmt.kind {
                for member in members.iter() {
                    if let ClassMember::Statement(inner_stmt) = member {
                        if let StatementKind::FunctionDecl {
                            name: func_name, ..
                        } = &inner_stmt.kind
                        {
                            sub_compiler.class_methods.insert(func_name.to_lowercase());
                        }
                    }
                }
            }
        }

        // Capture imports before compile consumes the sub-compiler.
        let sub_imports = sub_compiler.imports.clone();
        let mut chunk = sub_compiler.compile(&ast, &source)?;
        chunk.reconstruct_functions();

        // Pull js: import bindings from the sub-compiler into the parent so
        // they are available when the VM executes the parent chunk.
        self.propagate_js_imports(&sub_imports);

        for constant in chunk.constants {
            if let Constant::Class(_) = constant {
                return Ok(constant);
            }
        }

        bail!("No class declaration found in {}", file_path.display());
    }

    fn load_interface_from_path(&mut self, iface_path: &str) -> Result<Constant> {
        let file_path = self.resolve_source_file(iface_path)?;

        let source = fs::read_to_string(&file_path)?;
        let ast = crate::parser::parse(&source, file_path.to_str())
            .map_err(|e| anyhow::anyhow!("Parse Error in {}: {}", iface_path, e))?;

        let filename = file_path.to_str().unwrap_or(iface_path);
        let mut sub_compiler = Compiler::with_chunk(Chunk::new(filename));
        sub_compiler.imports = self.imports.clone();
        sub_compiler.module_paths = self.module_paths.clone();
        sub_compiler.source_dir = file_path.parent().map(|p| p.to_path_buf());
        sub_compiler.is_class = true;
        sub_compiler.current_line = self.current_line;
        sub_compiler.class_has_init_map = self.class_has_init_map.clone();

        let sub_imports = sub_compiler.imports.clone();
        let mut chunk = sub_compiler.compile(&ast, &source)?;
        chunk.reconstruct_functions();

        // Pull js: import bindings from the sub-compiler into the parent.
        self.propagate_js_imports(&sub_imports);

        for constant in chunk.constants {
            if let Constant::Interface(_) = constant {
                return Ok(constant);
            }
        }

        bail!("No interface declaration found in {}", file_path.display());
    }
}

pub struct DependencyTracker {
    pub used_symbols: HashSet<String>,
}

impl DependencyTracker {
    pub fn new() -> Self {
        Self {
            used_symbols: HashSet::new(),
        }
    }

    pub fn track_statements(&mut self, stmts: &[Statement]) {
        for stmt in stmts {
            self.track_statement(stmt);
        }
    }

    fn track_statement(&mut self, stmt: &Statement) {
        match &stmt.kind {
            StatementKind::FunctionDecl { body, .. } => {
                self.track_function_body(body);
            }
            StatementKind::ClassDecl { members, .. } => {
                for member in members {
                    match member {
                        ClassMember::Statement(s) => self.track_statement(s),
                        _ => {}
                    }
                }
            }
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.track_expression(condition);
                self.track_statements(then_branch);
                if let Some(eb) = else_branch {
                    self.track_statements(eb);
                }
            }
            StatementKind::ForLoop {
                collection, body, ..
            } => {
                self.track_expression(collection);
                self.track_statements(body);
            }
            StatementKind::ForClassic {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(i) = init {
                    self.track_statement(i);
                }
                if let Some(c) = condition {
                    self.track_expression(c);
                }
                if let Some(u) = update {
                    self.track_expression(u);
                }
                self.track_statements(body);
            }
            StatementKind::WhileLoop { condition, body } => {
                self.track_expression(condition);
                self.track_statements(body);
            }
            StatementKind::Switch {
                value,
                cases,
                default_case,
            } => {
                self.track_expression(value);
                for switch_case in cases {
                    self.track_expression(&switch_case.value);
                    self.track_statements(&switch_case.body);
                }
                if let Some(def) = default_case {
                    self.track_statements(def);
                }
            }
            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.track_expression(e);
                }
            }
            StatementKind::Throw(expr) => {
                if let Some(e) = expr {
                    self.track_expression(e);
                }
            }
            StatementKind::Break | StatementKind::Continue => {}
            StatementKind::VariableDecl { value, .. } => {
                self.track_expression(value);
            }
            StatementKind::Expression(expr) => {
                self.track_expression(expr);
            }
            StatementKind::TryCatch {
                try_branch,
                catches,
                finally_branch,
            } => {
                self.track_statements(try_branch);
                for catch in catches {
                    self.track_statements(&catch.body);
                }
                if let Some(fb) = finally_branch {
                    self.track_statements(fb);
                }
            }
            _ => {}
        }
    }

    pub fn track_expression(&mut self, expr: &Expression) {
        match &expr.kind {
            ExpressionKind::FunctionCall { base, args } => {
                if let ExpressionKind::Identifier(name) = &base.kind {
                    self.used_symbols.insert(name.to_lowercase());
                } else {
                    self.track_expression(base);
                }
                for arg in args {
                    self.track_expression(&arg.value);
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.track_expression(left);
                self.track_expression(right);
            }
            ExpressionKind::UnaryNot(inner) => {
                self.track_expression(inner);
            }
            ExpressionKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.track_expression(condition);
                self.track_expression(then_expr);
                self.track_expression(else_expr);
            }
            ExpressionKind::Elvis { left, right } => {
                self.track_expression(left);
                self.track_expression(right);
            }
            ExpressionKind::Assignment { value, .. } => {
                self.track_expression(value);
            }
            ExpressionKind::MemberAccess { base, .. } => {
                self.track_expression(base);
            }
            ExpressionKind::SafeMemberAccess { base, .. } => {
                self.track_expression(base);
            }
            ExpressionKind::ArrayAccess { base, index } => {
                self.track_expression(base);
                self.track_expression(index);
            }
            ExpressionKind::Literal(lit) => match lit {
                Literal::Array(items) => {
                    for item in items {
                        self.track_expression(item);
                    }
                }
                Literal::Struct(members) => {
                    for (k, v) in members {
                        self.track_expression(k);
                        self.track_expression(v);
                    }
                }
                Literal::Function { body, .. } => {
                    self.track_function_body(body);
                }
                _ => {}
            },
            ExpressionKind::Identifier(name) => {
                self.used_symbols.insert(name.to_lowercase());
            }
            _ => {}
        }
    }

    fn track_function_body(&mut self, body: &FunctionBody) {
        match body {
            FunctionBody::Block(stmts) => self.track_statements(stmts),
            FunctionBody::Expression(expr) => self.track_expression(expr),
            _ => {}
        }
    }
}
