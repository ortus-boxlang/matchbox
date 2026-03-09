use anyhow::{Result, bail};
use crate::ast::{Expression, ExpressionKind, Literal, Statement, StatementKind, StringPart, FunctionBody, ClassMember};
use matchbox_vm::types::{BxCompiledFunction, BxClass, BxInterface, Constant, box_string::BoxString};
use matchbox_vm::Chunk;
use matchbox_vm::vm::opcode::OpCode;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::fs;

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
    imports: HashMap<String, String>, // Alias -> Full Path
    current_line: u32,
    pub is_repl: bool,
    continue_patches: Vec<Vec<usize>>,
}

impl Compiler {
    pub fn new(filename: &str) -> Self {
        Compiler {
            chunk: Chunk::new(filename),
            locals: Vec::new(),
            scope_depth: 0,
            is_class: false,
            imports: HashMap::new(),
            current_line: 0,
            is_repl: false,
            continue_patches: Vec::new(),
        }
    }

    pub fn compile(mut self, ast: &[Statement], source: &str) -> Result<Chunk> {
        self.chunk.source = source.to_string();
        let len = ast.len();
        for (i, stmt) in ast.iter().enumerate() {
            let is_last = i == len - 1;
            self.compile_statement(stmt, is_last)?;
        }
        self.chunk.write(OpCode::OpReturn, self.current_line as u32);
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<()> {
        self.current_line = stmt.line as u32;
        match &stmt.kind {
            StatementKind::Import { path, alias } => {
                let resolved_alias = if let Some(a) = alias {
                    a.to_lowercase()
                } else {
                    path.split('.').last().unwrap().to_string().to_lowercase()
                };
                self.imports.insert(resolved_alias, path.clone());
                Ok(())
            }
            StatementKind::ClassDecl { name, extends, accessors, implements, members } => {
                let mut constructor_compiler = Compiler::new(&self.chunk.filename);
                constructor_compiler.is_class = true;
                constructor_compiler.scope_depth = 1;
                constructor_compiler.imports = self.imports.clone();
                constructor_compiler.current_line = stmt.line as u32;
                
                let mut methods = HashMap::new();
                let mut properties = Vec::new();
                
                for member in members {
                    match member {
                        ClassMember::Property(prop_name) => {
                            properties.push(prop_name.clone());
                            let null_idx = constructor_compiler.chunk.add_constant(Constant::Null);
                            constructor_compiler.chunk.write(OpCode::OpConstant(null_idx), stmt.line as u32);
                            let name_idx = constructor_compiler.chunk.add_constant(Constant::String(BoxString::new(&prop_name.clone())));
                            constructor_compiler.chunk.write(OpCode::OpSetPrivate(name_idx as u32), stmt.line as u32);
                            constructor_compiler.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                        }
                        ClassMember::Statement(inner_stmt) => {
                            match &inner_stmt.kind {
                                StatementKind::FunctionDecl { name: func_name, attributes: _, access_modifier: _, return_type: _, params, body } => {
                                    if let FunctionBody::Abstract = body {
                                        bail!("Abstract functions only allowed in interfaces");
                                    }
                                    let mut method_compiler = Compiler::new(&self.chunk.filename);
                                    method_compiler.is_class = true;
                                    method_compiler.imports = self.imports.clone();
                                    method_compiler.current_line = inner_stmt.line as u32;
                                    let func = method_compiler.compile_function(&func_name, &params, &body)?;
                                    methods.insert(func_name.to_lowercase(), Rc::new(func));
                                }
                                _ => {
                                    constructor_compiler.compile_statement(inner_stmt, false)?;
                                }
                            }
                        }
                    }
                }

                if *accessors {
                    for prop in &properties {
                        if prop.is_empty() { continue; }
                        let capitalized = format!("{}{}", &prop[..1].to_uppercase(), &prop[1..]);
                        
                        // Getter: getProp()
                        let getter_name = format!("get{}", capitalized);
                        if !methods.contains_key(&getter_name.to_lowercase()) {
                            let mut getter_chunk = Chunk::default();
                            getter_chunk.filename = self.chunk.filename.clone();
                            let name_idx = getter_chunk.add_constant(Constant::String(BoxString::new(&prop.clone())));
                            getter_chunk.write(OpCode::OpGetPrivate(name_idx as u32), stmt.line as u32);
                            getter_chunk.write(OpCode::OpReturn, stmt.line as u32);
                            
                            let constant_count = getter_chunk.constants.len();
                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, getter_name),
                                arity: 0,
                                min_arity: 0,
                                params: Vec::new(),
                                chunk: Rc::new(RefCell::new(getter_chunk)),
                                promoted_constants: RefCell::new(vec![None; constant_count]),
                            };
                            methods.insert(getter_name.to_lowercase(), Rc::new(func));
                        }

                        // Setter: setProp(val)
                        let setter_name = format!("set{}", capitalized);
                        if !methods.contains_key(&setter_name.to_lowercase()) {
                            let mut setter_chunk = Chunk::default();
                            setter_chunk.filename = self.chunk.filename.clone();
                            setter_chunk.write(OpCode::OpGetLocal(0), stmt.line as u32);
                            let name_idx = setter_chunk.add_constant(Constant::String(BoxString::new(&prop.clone())));
                            setter_chunk.write(OpCode::OpSetPrivate(name_idx as u32), stmt.line as u32);
                            setter_chunk.write(OpCode::OpReturn, stmt.line as u32);
                            
                            let constant_count = setter_chunk.constants.len();
                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, setter_name),
                                arity: 1,
                                min_arity: 1,
                                params: vec!["val".to_string()],
                                chunk: Rc::new(RefCell::new(setter_chunk)),
                                promoted_constants: RefCell::new(vec![None; constant_count]),
                            };
                            methods.insert(setter_name.to_lowercase(), Rc::new(func));
                        }
                    }
                }

                // Handle Interfaces (Traits and Contracts)
                for iface_name in implements {
                    let iface_val = if let Some(alias_path) = self.imports.get(&iface_name.to_lowercase()) {
                        let path = alias_path.clone();
                        self.load_interface_from_path(&path)?
                    } else {
                        // Look in local/global scope
                        // This POC assumes interfaces are in globals if not qualified
                        self.chunk.constants.iter().find_map(|c| {
                            if let Constant::Interface(i) = c {
                                if i.name.to_lowercase() == iface_name.to_lowercase() {
                                    return Some(Constant::Interface(i.clone()));
                                }
                            }
                            None
                        }).ok_or_else(|| anyhow::anyhow!("Interface {} not found", iface_name))?
                    };

                    if let Constant::Interface(iface) = iface_val {
                        for (method_name, method_opt) in &iface.methods {
                            if !methods.contains_key(method_name) {
                                if let Some(default_impl) = method_opt {
                                    methods.insert(method_name.clone(), Rc::clone(default_impl));
                                } else {
                                    bail!("Class {} must implement abstract method {} from interface {}", name, method_name, iface.name);
                                }
                            }
                        }
                    }
                }
                
                constructor_compiler.chunk.write(OpCode::OpReturn, stmt.line as u32 as u32);
                
                let constructor_constant_count = constructor_compiler.chunk.constants.len();
                let constructor = BxCompiledFunction {
                    name: format!("{}.constructor", name),
                    arity: 0,
                    min_arity: 0,
                    params: Vec::new(),
                    chunk: Rc::new(RefCell::new(constructor_compiler.chunk)),
                    promoted_constants: RefCell::new(vec![None; constructor_constant_count]),
                };

                let class = BxClass {
                    name: name.clone(),
                    extends: extends.as_ref().map(|s| s.to_lowercase()),
                    implements: implements.iter().map(|s| s.to_lowercase()).collect(),
                    constructor: Rc::new(constructor),
                    methods,
                };

                let class_idx = self.chunk.add_constant(Constant::Class(class));
                self.chunk.write(OpCode::OpConstant(class_idx), stmt.line as u32);
                let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx as u32), stmt.line as u32);
                Ok(())
                }            StatementKind::InterfaceDecl { name, members } => {
                let mut methods = HashMap::new();
                for member in members {
                    if let StatementKind::FunctionDecl { name: func_name, attributes: _, access_modifier: _, return_type: _, params, body } = &member.kind {
                        let method = if let FunctionBody::Abstract = body {
                            None
                        } else {
                            let mut method_compiler = Compiler::new(&self.chunk.filename);
                            method_compiler.is_class = true;
                            method_compiler.imports = self.imports.clone();
                            method_compiler.current_line = member.line;
                            let func = method_compiler.compile_function(func_name, params, body)?;
                            Some(Rc::new(func))
                        };
                        methods.insert(func_name.to_lowercase(), method);
                    } else {
                        bail!("Only function declarations allowed in interfaces");
                    }
                }
                let iface = BxInterface {
                    name: name.clone(),
                    methods,
                };
                let iface_idx = self.chunk.add_constant(Constant::Interface(iface));
                self.chunk.write(OpCode::OpConstant(iface_idx), stmt.line as u32);
                let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx as u32), stmt.line as u32);
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
                    self.chunk.write(OpCode::OpConstant(null_idx), stmt.line as u32);
                }
                self.chunk.write(OpCode::OpReturn, stmt.line as u32 as u32);
                Ok(())
            }
            StatementKind::Throw(expr) => {
                if let Some(e) = expr {
                    self.compile_expression(e)?;
                } else {
                    let null_idx = self.chunk.add_constant(Constant::Null);
                    self.chunk.write(OpCode::OpConstant(null_idx), stmt.line as u32);
                }
                self.chunk.write(OpCode::OpThrow, stmt.line as u32);
                Ok(())
            }
            StatementKind::Continue => {
                let jump_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0), stmt.line as u32);
                if let Some(patches) = self.continue_patches.last_mut() {
                    patches.push(jump_idx);
                } else {
                    bail!("'continue' used outside of a loop");
                }
                Ok(())
            }
            StatementKind::TryCatch { try_branch, catches, finally_branch } => {
                let push_handler_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpPushHandler(0), stmt.line as u32);

                self.begin_scope();
                for s in try_branch {
                    self.compile_statement(s, false)?;
                }
                self.end_scope();

                self.chunk.write(OpCode::OpPopHandler, stmt.line as u32);

                let jump_to_finally_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0 as u32), stmt.line as u32);

                let catch_target = self.chunk.code.len();
                let offset = catch_target - push_handler_idx - 1;
                self.chunk.code[push_handler_idx] = OpCode::OpPushHandler(offset as u32);

                if !catches.is_empty() {
                    let first_catch = &catches[0];
                    self.begin_scope();
                    self.add_local(first_catch.exception_var.clone());
                    for s in &first_catch.body {
                        self.compile_statement(s, false)?;
                    }
                    self.end_scope();
                } else {
                    self.chunk.write(OpCode::OpThrow, stmt.line as u32);
                }

                let finally_target = self.chunk.code.len();
                let jump_offset = finally_target - jump_to_finally_idx - 1;
                self.chunk.code[jump_to_finally_idx] = OpCode::OpJump(jump_offset as u32);

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
                        self.chunk.write(OpCode::OpDup, stmt.line as u32);
                    }
                    let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                    self.chunk.write(OpCode::OpDefineGlobal(name_idx as u32), stmt.line as u32);
                }
                Ok(())
            }
            StatementKind::If { condition, then_branch, else_branch } => {
                let mut optimized = false;
                let mut jump_if_false_idx = 0;

                if let ExpressionKind::Binary { left, operator, right } = &condition.kind {
                    if *operator == "==" {
                        if let (ExpressionKind::Identifier(name), ExpressionKind::Literal(lit)) = (&left.kind, &right.kind) {
                            if let Some(slot) = self.resolve_local(name) {
                                let const_idx = match lit {
                                    Literal::Number(n) => Some(self.chunk.add_constant(Constant::Number(*n))),
                                    Literal::Boolean(b) => Some(self.chunk.add_constant(Constant::Boolean(*b))),
                                    Literal::String(parts) if parts.len() == 1 => {
                                        if let StringPart::Text(t) = &parts[0] {
                                            Some(self.chunk.add_constant(Constant::String(BoxString::new(t))))
                                        } else { None }
                                    },
                                    Literal::Null => Some(self.chunk.add_constant(Constant::Null)),
                                    _ => None,
                                };
                                
                                if let Some(c_idx) = const_idx {
                                    jump_if_false_idx = self.chunk.code.len();
                                    self.chunk.write(OpCode::OpLocalJumpIfNeConst(slot as u32, c_idx as u32, 0), condition.line as u32);
                                    optimized = true;
                                }
                            }
                        }
                    }
                }

                if !optimized {
                    self.compile_expression(condition)?;
                    jump_if_false_idx = self.chunk.code.len();
                    self.chunk.write(OpCode::OpJumpIfFalse(0), stmt.line as u32);
                    self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                }

                self.begin_scope();
                for stmt in then_branch {
                    self.compile_statement(stmt, false)?;
                }
                self.end_scope();

                let jump_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0 as u32), stmt.line as u32);

                let false_target = self.chunk.code.len();
                let offset = false_target - jump_if_false_idx - 1;
                
                if optimized {
                    if let OpCode::OpLocalJumpIfNeConst(s, c, _) = self.chunk.code[jump_if_false_idx] {
                        self.chunk.code[jump_if_false_idx] = OpCode::OpLocalJumpIfNeConst(s, c, offset as u32);
                    }
                } else {
                    self.chunk.code[jump_if_false_idx] = OpCode::OpJumpIfFalse(offset as u32);
                    self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
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
                self.chunk.code[jump_idx] = OpCode::OpJump(jump_offset as u32);

                Ok(())
            }
            StatementKind::ForClassic { init, condition, update, body } => {
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
                    if let ExpressionKind::Binary { left, operator, right } = &cond_expr.kind {
                        if *operator == "<" {
                            if let (ExpressionKind::Identifier(name), ExpressionKind::Literal(Literal::Number(limit))) = (&left.kind, &right.kind) {
                                let const_idx = self.chunk.add_constant(Constant::Number(*limit));

                                if let Some(slot) = self.resolve_local(name) {
                                    // Local optimized candidate
                                    self.compile_expression(cond_expr)?;
                                    let jump_idx = self.chunk.code.len();
                                    self.chunk.write(OpCode::OpJumpIfFalse(0), cond_expr.line as u32);
                                    self.chunk.write(OpCode::OpPop, cond_expr.line as u32);
                                    exit_jump = Some(jump_idx);
                                    optimized_condition = Some((true, slot as u32, const_idx as u32)); // true = is_local
                                    handled = true;
                                } else if !self.is_class {
                                    // Global optimized candidate
                                    let name_lower = name.to_lowercase();
                                    let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name_lower)));
                                    
                                    // Initial check (standard instructions)
                                    self.compile_expression(cond_expr)?;
                                    let jump_idx = self.chunk.code.len();
                                    self.chunk.write(OpCode::OpJumpIfFalse(0), cond_expr.line as u32);
                                    self.chunk.write(OpCode::OpPop, cond_expr.line as u32);
                                    exit_jump = Some(jump_idx);
                                    optimized_condition = Some((false, name_idx as u32, const_idx as u32)); // false = is_local
                                    handled = true;
                                }
                            }
                        }
                    }

                    if !handled {
                        self.compile_expression(cond_expr)?;
                        let jump_idx = self.chunk.code.len();
                        self.chunk.write(OpCode::OpJumpIfFalse(0), stmt.line as u32);
                        self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                        exit_jump = Some(jump_idx);
                    }
                }

                let body_start = self.chunk.code.len();

                self.continue_patches.push(Vec::new());
                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }

                // Patch all continue jumps to land here (before the update step)
                let continue_target = self.chunk.code.len();
                if let Some(patches) = self.continue_patches.pop() {
                    for idx in patches {
                        let offset = continue_target - idx - 1;
                        self.chunk.code[idx] = OpCode::OpJump(offset as u32);
                    }
                }

                if let Some(update_expr) = update {
                    let mut optimized_update = false;
                    if let ExpressionKind::Postfix { base, operator } = &update_expr.kind {
                        if *operator == "++" {
                            if let ExpressionKind::Identifier(name) = &base.kind {
                                if let Some(slot) = self.resolve_local(name) {
                                    self.chunk.write(OpCode::OpIncLocal(slot as u32), update_expr.line);
                                    optimized_update = true;
                                } else if !self.is_class {
                                    let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                                    self.chunk.write(OpCode::OpIncGlobal(name_idx as u32), update_expr.line);
                                    optimized_update = true;
                                }
                            }
                        }
                    }
                    
                    if !optimized_update {
                        self.compile_expression(update_expr)?;
                        self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                    }
                }

                if let Some((is_local, idx, const_idx)) = optimized_condition {
                    if is_local {
                        let offset = self.chunk.code.len() - body_start + 1;
                        self.chunk.write(OpCode::OpLocalCompareJump(idx as u32, const_idx as u32, offset as u32), stmt.line as u32 as u32);
                    } else {
                        let offset = self.chunk.code.len() - body_start + 1;
                        self.chunk.write(OpCode::OpGlobalCompareJump(idx as u32, const_idx as u32, offset as u32), stmt.line as u32);
                    }
                } else {
                    let loop_end = self.chunk.code.len();
                    let offset = loop_end - loop_start + 1;
                    self.chunk.write(OpCode::OpLoop(offset as u32), stmt.line as u32);
                }

                if let Some(idx) = exit_jump {
                    let exit_target = self.chunk.code.len();
                    let offset = exit_target - idx - 1;
                    self.chunk.code[idx] = OpCode::OpJumpIfFalse(offset as u32);
                    self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                }
                self.end_scope();

                Ok(())
            }
            StatementKind::FunctionDecl { name, attributes: _, access_modifier: _, return_type: _, params, body } => {
                let func = self.compile_function(&name, &params, &body)?;
                if self.is_repl && is_last {
                    let func_idx = self.chunk.add_constant(Constant::CompiledFunction(func.clone()));
                    self.chunk.write(OpCode::OpConstant(func_idx), stmt.line as u32);
                }
                let func_idx = self.chunk.add_constant(Constant::CompiledFunction(func));
                self.chunk.write(OpCode::OpConstant(func_idx), stmt.line as u32);
                let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx as u32), stmt.line as u32);
                Ok(())
            }
            StatementKind::ForLoop { item, index, collection, body } => {
                self.begin_scope();
                
                self.compile_expression(collection)?;
                let collection_slot = self.locals.len();
                self.locals.push(Local { name: "$collection".to_string(), depth: self.scope_depth });

                let zero_idx = self.chunk.add_constant(Constant::Number(0.0));
                self.chunk.write(OpCode::OpConstant(zero_idx), stmt.line as u32);
                let cursor_slot = self.locals.len();
                self.locals.push(Local { name: "$cursor".to_string(), depth: self.scope_depth });

                let loop_start = self.chunk.code.len();

                let has_index = index.is_some();
                let iter_next_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpIterNext(collection_slot as u32, cursor_slot as u32, 0, has_index), stmt.line as u32 as u32);

                self.add_local(item.clone());
                if let Some(index_name) = index {
                    self.add_local(index_name.clone());
                }

                self.continue_patches.push(Vec::new());
                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }

                // Patch continue jumps to land here (before cleanup pops)
                let continue_target = self.chunk.code.len();
                if let Some(patches) = self.continue_patches.pop() {
                    for idx in patches {
                        let offset = continue_target - idx - 1;
                        self.chunk.code[idx] = OpCode::OpJump(offset as u32);
                    }
                }

                if index.is_some() {
                    self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                    self.locals.pop();
                }
                self.chunk.write(OpCode::OpPop, stmt.line as u32 as u32);
                self.locals.pop();

                let loop_end = self.chunk.code.len();
                let offset = loop_end - loop_start + 1;
                self.chunk.write(OpCode::OpLoop(offset as u32), stmt.line as u32);

                let exit_target = self.chunk.code.len();
                let offset = exit_target - iter_next_idx - 1;
                self.chunk.code[iter_next_idx] = OpCode::OpIterNext(collection_slot as u32, cursor_slot as u32, offset as u32, has_index);

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
                let resolved_path = self.imports.get(&lower_path).cloned().unwrap_or(class_path.clone());
                
                if resolved_path.to_lowercase().starts_with("java:") {
                    let java_class = &resolved_path[5..];
                    
                    // Push createObject onto stack
                    let bif_name_idx = self.chunk.add_constant(Constant::String(BoxString::new("createobject")));
                    self.chunk.write(OpCode::OpGetGlobal(bif_name_idx), expr.line);

                    // Push type "java"
                    let type_idx = self.chunk.add_constant(Constant::String(BoxString::new("java")));
                    self.chunk.write(OpCode::OpConstant(type_idx), expr.line);

                    // Push class name
                    let class_idx = self.chunk.add_constant(Constant::String(BoxString::new(java_class)));
                    self.chunk.write(OpCode::OpConstant(class_idx), expr.line);
                    
                    // Push arguments
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }
                    
                    // Call createObject(type, class, ...args)
                    self.chunk.write(OpCode::OpCall(2 + args.len() as u32), expr.line);
                    return Ok(());
                }

                if resolved_path.to_lowercase().starts_with("rust:") {
                    let rust_class = &resolved_path[5..];
                    
                    // Push createObject onto stack
                    let bif_name_idx = self.chunk.add_constant(Constant::String(BoxString::new("createobject")));
                    self.chunk.write(OpCode::OpGetGlobal(bif_name_idx), expr.line);

                    // Push type "rust"
                    let type_idx = self.chunk.add_constant(Constant::String(BoxString::new("rust")));
                    self.chunk.write(OpCode::OpConstant(type_idx), expr.line);

                    // Push class name
                    let class_idx = self.chunk.add_constant(Constant::String(BoxString::new(rust_class)));
                    self.chunk.write(OpCode::OpConstant(class_idx), expr.line);
                    
                    // Push arguments
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }
                    
                    // Call createObject(type, class, ...args)
                    self.chunk.write(OpCode::OpCall(2 + args.len() as u32), expr.line);
                    return Ok(());
                }

                if resolved_path.contains('.') {
                    let class_val = self.load_class_from_path(&resolved_path)?;
                    let class_idx = self.chunk.add_constant(class_val);
                    self.chunk.write(OpCode::OpConstant(class_idx), expr.line);
                } else {
                    let class_idx = self.chunk.add_constant(Constant::String(BoxString::new(&resolved_path)));
                    self.chunk.write(OpCode::OpGetGlobal(class_idx), expr.line);
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
                self.chunk.write(OpCode::OpNew(args.len() as u32), expr.line);
                
                // Automatically call init() if args were passed
                if !args.is_empty() {
                    let name_idx = self.chunk.add_constant(Constant::String(BoxString::new("init")));
                    if has_named {
                        let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                        self.chunk.write(OpCode::OpInvokeNamed(name_idx as u32, args.len() as u32, names_idx as u32), expr.line);
                    } else {
                        self.chunk.write(OpCode::OpInvoke(name_idx as u32, args.len() as u32), expr.line);
                    }
                }
                
                Ok(())
            }
            ExpressionKind::Literal(lit) => match lit {
                Literal::Number(n) => {
                    let idx = self.chunk.add_constant(Constant::Number(*n));
                    self.chunk.write(OpCode::OpConstant(idx as u32), expr.line);
                    Ok(())
                }
                Literal::String(parts) => {
                    if parts.is_empty() {
                        let idx = self.chunk.add_constant(Constant::String(BoxString::new("")));
                        self.chunk.write(OpCode::OpConstant(idx as u32), expr.line);
                        return Ok(());
                    }
                    self.compile_string_part(&parts[0])?;
                    for i in 1..parts.len() {
                        self.compile_string_part(&parts[i])?;
                        self.chunk.write(OpCode::OpStringConcat, expr.line);
                    }
                    Ok(())
                }
                Literal::Boolean(b) => {
                    let idx = self.chunk.add_constant(Constant::Boolean(*b));
                    self.chunk.write(OpCode::OpConstant(idx as u32), expr.line);
                    Ok(())
                }
                Literal::Null => {
                    let idx = self.chunk.add_constant(Constant::Null);
                    self.chunk.write(OpCode::OpConstant(idx as u32), expr.line);
                    Ok(())
                }
                Literal::Array(items) => {
                    for item in items {
                        self.compile_expression(item)?;
                    }
                    self.chunk.write(OpCode::OpArray(items.len() as u32), expr.line);
                    Ok(())
                }
                Literal::Struct(members) => {
                    for (key_expr, val_expr) in members {
                        match &key_expr.kind {
                            ExpressionKind::Identifier(name) => {
                                let idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                                self.chunk.write(OpCode::OpConstant(idx as u32), expr.line);
                            }
                            _ => self.compile_expression(key_expr)?,
                        }
                        self.compile_expression(val_expr)?;
                    }
                    self.chunk.write(OpCode::OpStruct(members.len() as u32), expr.line);
                    Ok(())
                }
                Literal::Function { params, body } => {
                    let func = self.compile_function("anonymous", &params, &body)?;
                    let func_idx = self.chunk.add_constant(Constant::CompiledFunction(func));
                    self.chunk.write(OpCode::OpConstant(func_idx), expr.line);
                    Ok(())
                }
            },
            ExpressionKind::Binary { left, operator, right } => {
                // Short-circuit operators handle their own right-side compilation
                match operator.as_str() {
                    "||" => {
                        self.compile_expression(left)?;
                        // If left is falsy, skip the unconditional jump to land at pop+right
                        let jif_idx = self.chunk.code.len();
                        self.chunk.write(OpCode::OpJumpIfFalse(0), expr.line);
                        // Left is truthy: jump past pop+right to end
                        let jump_idx = self.chunk.code.len();
                        self.chunk.write(OpCode::OpJump(0), expr.line);
                        // False path: pop falsy left, evaluate right
                        let false_target = self.chunk.code.len();
                        self.chunk.code[jif_idx] = OpCode::OpJumpIfFalse((false_target - jif_idx - 1) as u32);
                        self.chunk.write(OpCode::OpPop, expr.line);
                        self.compile_expression(right)?;
                        // Patch unconditional jump to end
                        let end_target = self.chunk.code.len();
                        self.chunk.code[jump_idx] = OpCode::OpJump((end_target - jump_idx - 1) as u32);
                        return Ok(());
                    }
                    "&&" => {
                        self.compile_expression(left)?;
                        // If left is falsy, jump to end (left stays on stack as result)
                        let jif_idx = self.chunk.code.len();
                        self.chunk.write(OpCode::OpJumpIfFalse(0), expr.line);
                        // Left is truthy: pop it, evaluate right
                        self.chunk.write(OpCode::OpPop, expr.line);
                        self.compile_expression(right)?;
                        // Patch JumpIfFalse to end
                        let end_target = self.chunk.code.len();
                        self.chunk.code[jif_idx] = OpCode::OpJumpIfFalse((end_target - jif_idx - 1) as u32);
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
                        if let (ExpressionKind::Literal(Literal::Number(a)), ExpressionKind::Literal(Literal::Number(b))) = (&left.kind, &right.kind) {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.write(OpCode::OpAddInt, expr.line);
                            } else {
                                self.chunk.write(OpCode::OpAddFloat, expr.line);
                            }
                            specialized = true;
                        }
                        
                        if !specialized {
                            self.chunk.write(OpCode::OpAdd, expr.line);
                        }
                    }
                    "-" => {
                        let mut specialized = false;
                        if let (ExpressionKind::Literal(Literal::Number(a)), ExpressionKind::Literal(Literal::Number(b))) = (&left.kind, &right.kind) {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.write(OpCode::OpSubInt, expr.line);
                            } else {
                                self.chunk.write(OpCode::OpSubFloat, expr.line);
                            }
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.write(OpCode::OpSubtract, expr.line);
                        }
                    }
                    "*" => {
                        let mut specialized = false;
                        if let (ExpressionKind::Literal(Literal::Number(a)), ExpressionKind::Literal(Literal::Number(b))) = (&left.kind, &right.kind) {
                            if a.fract() == 0.0 && b.fract() == 0.0 {
                                self.chunk.write(OpCode::OpMulInt, expr.line);
                            } else {
                                self.chunk.write(OpCode::OpMulFloat, expr.line);
                            }
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.write(OpCode::OpMultiply, expr.line);
                        }
                    }
                    "/" => {
                        let mut specialized = false;
                        if let (ExpressionKind::Literal(Literal::Number(_)), ExpressionKind::Literal(Literal::Number(_))) = (&left.kind, &right.kind) {
                            self.chunk.write(OpCode::OpDivFloat, expr.line);
                            specialized = true;
                        }
                        if !specialized {
                            self.chunk.write(OpCode::OpDivide, expr.line);
                        }
                    }
                    "&" => self.chunk.write(OpCode::OpStringConcat, expr.line),
                    "==" => self.chunk.write(OpCode::OpEqual, expr.line),
                    "!=" => self.chunk.write(OpCode::OpNotEqual, expr.line),
                    "<" => self.chunk.write(OpCode::OpLess, expr.line),
                    "<=" => self.chunk.write(OpCode::OpLessEqual, expr.line),
                    ">" => self.chunk.write(OpCode::OpGreater, expr.line),
                    ">=" => self.chunk.write(OpCode::OpGreaterEqual, expr.line),
                    _ => bail!("Unknown operator: {}", operator),
                }
                Ok(())
            }
            ExpressionKind::UnaryNot(inner) => {
                self.compile_expression(inner)?;
                self.chunk.write(OpCode::OpNot, expr.line);
                Ok(())
            }
            ExpressionKind::Ternary { condition, then_expr, else_expr } => {
                // Compile condition
                self.compile_expression(condition)?;
                // Jump to else branch if false
                let jif_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJumpIfFalse(0), expr.line);
                // True branch
                self.chunk.write(OpCode::OpPop, expr.line);
                self.compile_expression(then_expr)?;
                // Jump over else branch
                let jump_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0), expr.line);
                // False branch
                let else_target = self.chunk.code.len();
                self.chunk.code[jif_idx] = OpCode::OpJumpIfFalse((else_target - jif_idx - 1) as u32);
                self.chunk.write(OpCode::OpPop, expr.line);
                self.compile_expression(else_expr)?;
                // Patch end jump
                let end_target = self.chunk.code.len();
                self.chunk.code[jump_idx] = OpCode::OpJump((end_target - jump_idx - 1) as u32);
                Ok(())
            }
            ExpressionKind::Identifier(name) => {
                let lower_name = name.to_lowercase();
                if lower_name == "this" {
                    let idx = self.chunk.add_constant(Constant::String(BoxString::new("this")));
                    self.chunk.write(OpCode::OpGetPrivate(idx as u32), expr.line);
                } else if lower_name == "variables" {
                    let idx = self.chunk.add_constant(Constant::String(BoxString::new("variables")));
                    self.chunk.write(OpCode::OpGetPrivate(idx as u32), expr.line);
                } else if let Some(slot) = self.resolve_local(&name) {
                    self.chunk.write(OpCode::OpGetLocal(slot as u32), expr.line);
                } else if self.is_class {
                    let idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                    self.chunk.write(OpCode::OpGetPrivate(idx as u32), expr.line);
                } else {
                    let idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                    self.chunk.write(OpCode::OpGetGlobal(idx as u32), expr.line);
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
                        self.compile_expression(value)?;
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.write(OpCode::OpSetLocal(slot as u32), expr.line);
                        } else if self.is_class {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                            self.chunk.write(OpCode::OpSetPrivate(name_idx as u32), expr.line);
                        } else {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                            self.chunk.write(OpCode::OpSetGlobal(name_idx), expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.compile_expression(value)?;
                        let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                        self.chunk.write(OpCode::OpSetMember(name_idx as u32), expr.line);
                    }
                    crate::ast::AssignmentTarget::Index { base, index } => {
                        self.compile_expression(base)?;
                        self.compile_expression(index)?;
                        self.compile_expression(value)?;
                        self.chunk.write(OpCode::OpSetIndex, expr.line);
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
                        self.chunk.write(OpCode::OpPrintln(args.len() as u32), expr.line);
                        let null_idx = self.chunk.add_constant(Constant::Null);
                        self.chunk.write(OpCode::OpConstant(null_idx), expr.line);
                        return Ok(());
                    }
                    if lower_name == "print" {
                        for arg in args {
                            self.compile_expression(&arg.value)?;
                        }
                        self.chunk.write(OpCode::OpPrint(args.len() as u32), expr.line);
                        let null_idx = self.chunk.add_constant(Constant::Null);
                        self.chunk.write(OpCode::OpConstant(null_idx), expr.line);
                        return Ok(());
                    }
                }
                
                if let ExpressionKind::MemberAccess { base: member_base, member } = &base.kind {
                    self.compile_expression(member_base)?;
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }
                    let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                    if has_named {
                        let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                        self.chunk.write(OpCode::OpInvokeNamed(name_idx as u32, args.len() as u32, names_idx as u32), expr.line);
                    } else {
                        self.chunk.write(OpCode::OpInvoke(name_idx as u32, args.len() as u32), expr.line);
                    }
                    return Ok(());
                }

                self.compile_expression(base)?;
                for arg in args {
                    self.compile_expression(&arg.value)?;
                }
                if has_named {
                    let names_idx = self.chunk.add_constant(Constant::StringArray(arg_names));
                    self.chunk.write(OpCode::OpCallNamed(args.len() as u32, names_idx as u32), expr.line);
                } else {
                    self.chunk.write(OpCode::OpCall(args.len() as u32), expr.line);
                }
                Ok(())
            }
            ExpressionKind::ArrayAccess { base, index } => {
                self.compile_expression(base)?;
                self.compile_expression(index)?;
                self.chunk.write(OpCode::OpIndex, expr.line);
                Ok(())
            }
            ExpressionKind::MemberAccess { base, member } => {
                self.compile_expression(base)?;
                let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                self.chunk.write(OpCode::OpMember(name_idx), expr.line);
                Ok(())
            }
            ExpressionKind::Prefix { operator, target } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        self.compile_expression(&Expression::new(ExpressionKind::Identifier(name.clone()), expr.line))?;
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.write(OpCode::OpSetLocal(slot as u32), expr.line);
                        } else if self.is_class {
                            let idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            self.chunk.write(OpCode::OpSetPrivate(idx as u32), expr.line);
                        } else {
                            let idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            self.chunk.write(OpCode::OpSetGlobal(idx as u32), expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.chunk.write(OpCode::OpDup, expr.line);
                        let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                        self.chunk.write(OpCode::OpMember(name_idx), expr.line);
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        self.chunk.write(OpCode::OpSetMember(name_idx as u32), expr.line);
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
                        self.chunk.write(OpCode::OpDup, expr.line);
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        if let Some(slot) = self.resolve_local(&name) {
                            self.chunk.write(OpCode::OpSetLocal(slot as u32), expr.line);
                        } else if self.is_class {
                            let idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            self.chunk.write(OpCode::OpSetPrivate(idx as u32), expr.line);
                        } else {
                            let idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            self.chunk.write(OpCode::OpSetGlobal(idx as u32), expr.line);
                        }
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
                    }
                    ExpressionKind::MemberAccess { base: member_base, member } => {
                        self.compile_expression(member_base)?;
                        self.chunk.write(OpCode::OpDup, expr.line);
                        let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                        self.chunk.write(OpCode::OpMember(name_idx), expr.line);
                        self.chunk.write(OpCode::OpSwap, expr.line);
                        self.chunk.write(OpCode::OpOver, expr.line);
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        self.chunk.write(OpCode::OpSetMember(name_idx as u32), expr.line);
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
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
                let idx = self.chunk.add_constant(Constant::String(BoxString::new(&t.clone())));
                self.chunk.write(OpCode::OpConstant(idx as u32), self.current_line);
                Ok(())
            }
            StringPart::Expression(expr) => self.compile_expression(expr),
        }
    }

    fn compile_function(&mut self, name: &str, params: &[crate::ast::FunctionParam], body: &crate::ast::FunctionBody) -> Result<BxCompiledFunction> {
        let mut sub_compiler = Compiler::new(&self.chunk.filename);
        // Source text lives only in the root chunk to avoid N copies per file.
        // The VM falls back to disk when chunk.source is empty.
        sub_compiler.scope_depth = 1;
        sub_compiler.is_class = self.is_class;
        sub_compiler.imports = self.imports.clone();
        sub_compiler.current_line = self.current_line;

        let mut min_arity = 0;
        for (i, param) in params.iter().enumerate() {
            if param.required {
                min_arity = (i + 1) as u32;
            }
            sub_compiler.locals.push(Local {
                name: param.name.clone(),
                depth: 1,
            });
        }

        // Emit default value logic at the beginning of the function
        for (i, param) in params.iter().enumerate() {
            if let Some(default_expr) = &param.default_value {
                sub_compiler.chunk.write(OpCode::OpGetLocal(i as u32), self.current_line);
                let null_idx = sub_compiler.chunk.add_constant(Constant::Null);
                sub_compiler.chunk.write(OpCode::OpConstant(null_idx), self.current_line);
                sub_compiler.chunk.write(OpCode::OpEqual, self.current_line);
                
                let jump_idx = sub_compiler.chunk.code.len();
                sub_compiler.chunk.write(OpCode::OpJumpIfFalse(0), self.current_line);
                
                // True branch: value IS null
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop true
                sub_compiler.compile_expression(default_expr)?;
                sub_compiler.chunk.write(OpCode::OpSetLocal(i as u32), self.current_line);
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop set value
                
                let end_jump_idx = sub_compiler.chunk.code.len();
                sub_compiler.chunk.write(OpCode::OpJump(0 as u32), self.current_line);

                // False target: value IS NOT null
                let false_target = sub_compiler.chunk.code.len();
                let offset = false_target - jump_idx - 1;
                sub_compiler.chunk.code[jump_idx] = OpCode::OpJumpIfFalse(offset as u32);
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop false

                let end_target = sub_compiler.chunk.code.len();
                let end_offset = end_target - end_jump_idx - 1;
                sub_compiler.chunk.code[end_jump_idx] = OpCode::OpJump(end_offset as u32);
            }
        }

        match body {
            FunctionBody::Block(stmts) => {
                for stmt in stmts {
                    sub_compiler.compile_statement(stmt, false)?;
                }
                let null_idx = sub_compiler.chunk.add_constant(Constant::Null);
                sub_compiler.chunk.write(OpCode::OpConstant(null_idx), sub_compiler.current_line);
                sub_compiler.chunk.write(OpCode::OpReturn, sub_compiler.current_line);
            }
            FunctionBody::Expression(expr) => {
                sub_compiler.compile_expression(expr)?;
                sub_compiler.chunk.write(OpCode::OpReturn, sub_compiler.current_line);
            }
            FunctionBody::Abstract => {
                bail!("Cannot compile an abstract function body");
            }
        }

        let constant_count = sub_compiler.chunk.constants.len();
        Ok(BxCompiledFunction {
            name: name.to_string(),
            arity: params.len() as u32,
            min_arity,
            params: params.iter().map(|p| p.name.to_lowercase()).collect(),
            chunk: Rc::new(RefCell::new(sub_compiler.chunk)),
            promoted_constants: RefCell::new(vec![None; constant_count]),
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
                            self.chunk.write(OpCode::OpSetLocalPop(slot as u32), expr.line);
                        } else if self.is_class {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                            self.chunk.write(OpCode::OpSetPrivate(name_idx as u32), expr.line);
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        } else {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&lower_name)));
                            self.chunk.write(OpCode::OpSetGlobalPop(name_idx as u32), expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.compile_expression(value)?;
                        let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                        self.chunk.write(OpCode::OpSetMember(name_idx as u32), expr.line);
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
                    }
                    crate::ast::AssignmentTarget::Index { base, index } => {
                        self.compile_expression(base)?;
                        self.compile_expression(index)?;
                        self.compile_expression(value)?;
                        self.chunk.write(OpCode::OpSetIndex, expr.line);
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
                    }
                }
                Ok(())
            }
            ExpressionKind::Postfix { base, operator } => {
                match &base.kind {
                    ExpressionKind::Identifier(name) => {
                        if let Some(slot) = self.resolve_local(&name) {
                            if *operator == "++" {
                                self.chunk.write(OpCode::OpIncLocal(slot as u32), expr.line);
                            } else {
                                self.compile_expression(base)?;
                                self.chunk.write(OpCode::OpDec, expr.line);
                                self.chunk.write(OpCode::OpSetLocalPop(slot as u32), expr.line);
                            }
                        } else if !self.is_class {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            if *operator == "++" {
                                self.chunk.write(OpCode::OpIncGlobal(name_idx as u32), expr.line);
                            } else {
                                self.compile_expression(base)?;
                                self.chunk.write(OpCode::OpDec, expr.line);
                                self.chunk.write(OpCode::OpSetGlobalPop(name_idx as u32), expr.line);
                            }
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        }
                    }
                    ExpressionKind::MemberAccess { base: member_base, member } => {
                        if *operator == "++" {
                            self.compile_expression(member_base)?;
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                            self.chunk.write(OpCode::OpIncMember(name_idx as u32), expr.line);
                            self.chunk.write(OpCode::OpPop, expr.line as u32); // OpIncMember pushes NEW value, we pop it
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        }
                    }
                    _ => {
                        self.compile_expression(expr)?;
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
                    }
                }
                Ok(())
            }
            ExpressionKind::Prefix { operator, target } => {
                match target {
                    crate::ast::AssignmentTarget::Identifier(name) => {
                        if let Some(slot) = self.resolve_local(&name) {
                            if *operator == "++" {
                                self.chunk.write(OpCode::OpIncLocal(slot as u32), expr.line);
                            } else {
                                self.compile_expression(&Expression::new(ExpressionKind::Identifier(name.clone()), expr.line))?;
                                self.chunk.write(OpCode::OpDec, expr.line);
                                self.chunk.write(OpCode::OpSetLocalPop(slot as u32), expr.line);
                            }
                        } else if !self.is_class {
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&name.to_lowercase())));
                            if *operator == "++" {
                                self.chunk.write(OpCode::OpIncGlobal(name_idx as u32), expr.line);
                            } else {
                                self.compile_expression(&Expression::new(ExpressionKind::Identifier(name.clone()), expr.line))?;
                                self.chunk.write(OpCode::OpDec, expr.line);
                                self.chunk.write(OpCode::OpSetGlobalPop(name_idx as u32), expr.line);
                            }
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base: member_base, member } => {
                        if *operator == "++" {
                            self.compile_expression(member_base)?;
                            let name_idx = self.chunk.add_constant(Constant::String(BoxString::new(&member.to_lowercase())));
                            self.chunk.write(OpCode::OpIncMember(name_idx as u32), expr.line);
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        } else {
                            self.compile_expression(expr)?;
                            self.chunk.write(OpCode::OpPop, expr.line as u32);
                        }
                    }
                    _ => {
                        self.compile_expression(expr)?;
                        self.chunk.write(OpCode::OpPop, expr.line as u32);
                    }
                }
                Ok(())
            }
            _ => {
                self.compile_expression(expr)?;
                self.chunk.write(OpCode::OpPop, expr.line as u32);
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
                self.chunk.write(OpCode::OpPop, self.current_line);
            } else {
                break;
            }
        }
    }

    fn load_class_from_path(&mut self, class_path: &str) -> Result<Constant> {
        let rel_path = class_path.replace('.', "/") + ".bxs";
        let path = Path::new(&rel_path);
        
        if !path.exists() {
            bail!("Class file not found: {}", path.display());
        }
        
        let source = fs::read_to_string(path)?;
        let ast = crate::parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error in {}: {}", class_path, e))?;
        
        let mut sub_compiler = Compiler::new(&rel_path);
        sub_compiler.imports = self.imports.clone();
        sub_compiler.is_class = true; 
        sub_compiler.current_line = self.current_line;
        
        let chunk = sub_compiler.compile(&ast, &source)?;
        
        for constant in chunk.constants {
            if let Constant::Class(_) = constant {
                return Ok(constant);
            }
        }
        
        bail!("No class declaration found in {}", path.display());
    }

    fn load_interface_from_path(&mut self, iface_path: &str) -> Result<Constant> {
        let rel_path = iface_path.replace('.', "/") + ".bxs";
        let path = Path::new(&rel_path);
        
        if !path.exists() {
            bail!("Interface file not found: {}", path.display());
        }
        
        let source = fs::read_to_string(path)?;
        let ast = crate::parser::parse(&source).map_err(|e| anyhow::anyhow!("Parse Error in {}: {}", iface_path, e))?;
        
        let mut sub_compiler = Compiler::new(&rel_path);
        sub_compiler.imports = self.imports.clone();
        sub_compiler.is_class = true; 
        sub_compiler.current_line = self.current_line;
        
        let chunk = sub_compiler.compile(&ast, &source)?;
        
        for constant in chunk.constants {
            if let Constant::Interface(_) = constant {
                return Ok(constant);
            }
        }
        
        bail!("No interface declaration found in {}", path.display());
    }
}

use std::collections::HashSet;

pub struct DependencyTracker {
    pub used_symbols: HashSet<String>,
}

impl DependencyTracker {
    pub fn new() -> Self {
        Self { used_symbols: HashSet::new() }
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
            StatementKind::If { condition, then_branch, else_branch } => {
                self.track_expression(condition);
                self.track_statements(then_branch);
                if let Some(eb) = else_branch {
                    self.track_statements(eb);
                }
            }
            StatementKind::ForLoop { collection, body, .. } => {
                self.track_expression(collection);
                self.track_statements(body);
            }
            StatementKind::ForClassic { init, condition, update, body } => {
                if let Some(i) = init { self.track_statement(i); }
                if let Some(c) = condition { self.track_expression(c); }
                if let Some(u) = update { self.track_expression(u); }
                self.track_statements(body);
            }
            StatementKind::Return(expr) => {
                if let Some(e) = expr { self.track_expression(e); }
            }
            StatementKind::VariableDecl { value, .. } => {
                self.track_expression(value);
            }
            StatementKind::Expression(expr) => {
                self.track_expression(expr);
            }
            StatementKind::TryCatch { try_branch, catches, finally_branch } => {
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
            ExpressionKind::Ternary { condition, then_expr, else_expr } => {
                self.track_expression(condition);
                self.track_expression(then_expr);
                self.track_expression(else_expr);
            }
            ExpressionKind::Assignment { value, .. } => {
                self.track_expression(value);
            }
            ExpressionKind::MemberAccess { base, .. } => {
                self.track_expression(base);
            }
            ExpressionKind::ArrayAccess { base, index } => {
                self.track_expression(base);
                self.track_expression(index);
            }
            ExpressionKind::Literal(lit) => {
                match lit {
                    Literal::Array(items) => {
                        for item in items { self.track_expression(item); }
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
                }
            }
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
