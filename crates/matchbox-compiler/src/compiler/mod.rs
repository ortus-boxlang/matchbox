use anyhow::{Result, bail};
use crate::ast::{Expression, ExpressionKind, Literal, Statement, StatementKind, StringPart, FunctionBody, ClassMember};
use matchbox_vm::types::{BxValue, BxCompiledFunction, BxClass, BxInterface};
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
    current_line: usize,
    pub is_repl: bool,
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
        }
    }

    pub fn compile(mut self, ast: &[Statement], source: &str) -> Result<Chunk> {
        self.chunk.source = source.to_string();
        let len = ast.len();
        for (i, stmt) in ast.iter().enumerate() {
            let is_last = i == len - 1;
            self.compile_statement(stmt, is_last)?;
        }
        self.chunk.write(OpCode::OpReturn, self.current_line);
        Ok(self.chunk)
    }

    fn compile_statement(&mut self, stmt: &Statement, is_last: bool) -> Result<()> {
        self.current_line = stmt.line;
        match &stmt.kind {
            StatementKind::Import(path) => {
                let alias = path.split('.').last().unwrap().to_string().to_lowercase();
                self.imports.insert(alias, path.clone());
                Ok(())
            }
            StatementKind::ClassDecl { name, extends, accessors, implements, members } => {
                let mut constructor_compiler = Compiler::new(&self.chunk.filename);
                constructor_compiler.is_class = true;
                constructor_compiler.scope_depth = 1;
                constructor_compiler.imports = self.imports.clone();
                constructor_compiler.current_line = stmt.line;
                
                let mut methods = HashMap::new();
                let mut properties = Vec::new();
                
                for member in members {
                    match member {
                        ClassMember::Property(prop_name) => {
                            properties.push(prop_name.clone());
                            let null_idx = constructor_compiler.chunk.add_constant(BxValue::Null);
                            constructor_compiler.chunk.write(OpCode::OpConstant(null_idx), stmt.line);
                            let name_idx = constructor_compiler.chunk.add_constant(BxValue::String(prop_name.clone()));
                            constructor_compiler.chunk.write(OpCode::OpSetPrivate(name_idx), stmt.line);
                            constructor_compiler.chunk.write(OpCode::OpPop, stmt.line);
                        }
                        ClassMember::Statement(inner_stmt) => {
                            match &inner_stmt.kind {
                                StatementKind::FunctionDecl { name: func_name, access_modifier: _, return_type: _, params, body } => {
                                    if let FunctionBody::Abstract = body {
                                        bail!("Abstract functions only allowed in interfaces");
                                    }
                                    let mut method_compiler = Compiler::new(&self.chunk.filename);
                                    method_compiler.is_class = true;
                                    method_compiler.imports = self.imports.clone();
                                    method_compiler.current_line = inner_stmt.line;
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
                            let name_idx = getter_chunk.add_constant(BxValue::String(prop.clone()));
                            getter_chunk.write(OpCode::OpGetPrivate(name_idx), stmt.line);
                            getter_chunk.write(OpCode::OpReturn, stmt.line);
                            
                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, getter_name),
                                arity: 0,
                                min_arity: 0,
                                params: Vec::new(),
                                chunk: Rc::new(RefCell::new(getter_chunk)),
                            };
                            methods.insert(getter_name.to_lowercase(), Rc::new(func));
                        }

                        // Setter: setProp(val)
                        let setter_name = format!("set{}", capitalized);
                        if !methods.contains_key(&setter_name.to_lowercase()) {
                            let mut setter_chunk = Chunk::default();
                            setter_chunk.filename = self.chunk.filename.clone();
                            setter_chunk.write(OpCode::OpGetLocal(0), stmt.line);
                            let name_idx = setter_chunk.add_constant(BxValue::String(prop.clone()));
                            setter_chunk.write(OpCode::OpSetPrivate(name_idx), stmt.line);
                            setter_chunk.write(OpCode::OpReturn, stmt.line);
                            
                            let func = BxCompiledFunction {
                                name: format!("{}.{}", name, setter_name),
                                arity: 1,
                                min_arity: 1,
                                params: vec!["val".to_string()],
                                chunk: Rc::new(RefCell::new(setter_chunk)),
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
                            if let BxValue::Interface(i) = c {
                                if i.borrow().name.to_lowercase() == iface_name.to_lowercase() {
                                    return Some(BxValue::Interface(Rc::clone(i)));
                                }
                            }
                            None
                        }).ok_or_else(|| anyhow::anyhow!("Interface {} not found", iface_name))?
                    };

                    if let BxValue::Interface(iface) = iface_val {
                        let iface_ref = iface.borrow();
                        for (method_name, method_opt) in &iface_ref.methods {
                            if !methods.contains_key(method_name) {
                                if let Some(default_impl) = method_opt {
                                    methods.insert(method_name.clone(), Rc::clone(default_impl));
                                } else {
                                    bail!("Class {} must implement abstract method {} from interface {}", name, method_name, iface_ref.name);
                                }
                            }
                        }
                    }
                }
                
                constructor_compiler.chunk.write(OpCode::OpReturn, stmt.line);
                
                let class = BxClass {
                    name: name.clone(),
                    extends: extends.clone(),
                    implements: implements.clone(),
                    constructor: Rc::new(RefCell::new(constructor_compiler.chunk)),
                    methods,
                };
                
                let class_idx = self.chunk.add_constant(BxValue::Class(Rc::new(RefCell::new(class))));
                self.chunk.write(OpCode::OpConstant(class_idx), stmt.line);
                let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx), stmt.line);
                Ok(())
            }
            StatementKind::InterfaceDecl { name, members } => {
                let mut methods = HashMap::new();
                for member in members {
                    if let StatementKind::FunctionDecl { name: func_name, access_modifier: _, return_type: _, params, body } = &member.kind {
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
                let iface_idx = self.chunk.add_constant(BxValue::Interface(Rc::new(RefCell::new(iface))));
                self.chunk.write(OpCode::OpConstant(iface_idx), stmt.line);
                let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx), stmt.line);
                Ok(())
            }
            StatementKind::Expression(expr) => {
                self.compile_expression(expr)?;
                if !self.is_repl || !is_last {
                    self.chunk.write(OpCode::OpPop, stmt.line);
                }
                Ok(())
            }
            StatementKind::Return(expr) => {
                if let Some(e) = expr {
                    self.compile_expression(e)?;
                } else {
                    let null_idx = self.chunk.add_constant(BxValue::Null);
                    self.chunk.write(OpCode::OpConstant(null_idx), stmt.line);
                }
                self.chunk.write(OpCode::OpReturn, stmt.line);
                Ok(())
            }
            StatementKind::Throw(expr) => {
                if let Some(e) = expr {
                    self.compile_expression(e)?;
                } else {
                    let null_idx = self.chunk.add_constant(BxValue::Null);
                    self.chunk.write(OpCode::OpConstant(null_idx), stmt.line);
                }
                self.chunk.write(OpCode::OpThrow, stmt.line);
                Ok(())
            }
            StatementKind::TryCatch { try_branch, catches, finally_branch } => {
                let push_handler_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpPushHandler(0), stmt.line);

                self.begin_scope();
                for s in try_branch {
                    self.compile_statement(s, false)?;
                }
                self.end_scope();

                self.chunk.write(OpCode::OpPopHandler, stmt.line);

                let jump_to_finally_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0), stmt.line);

                let catch_target = self.chunk.code.len();
                let offset = catch_target - push_handler_idx - 1;
                self.chunk.code[push_handler_idx] = OpCode::OpPushHandler(offset);

                if !catches.is_empty() {
                    let first_catch = &catches[0];
                    self.begin_scope();
                    self.add_local(first_catch.exception_var.clone());
                    for s in &first_catch.body {
                        self.compile_statement(s, false)?;
                    }
                    self.end_scope();
                } else {
                    self.chunk.write(OpCode::OpThrow, stmt.line);
                }

                let finally_target = self.chunk.code.len();
                let jump_offset = finally_target - jump_to_finally_idx - 1;
                self.chunk.code[jump_to_finally_idx] = OpCode::OpJump(jump_offset);

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
                        self.chunk.write(OpCode::OpDup, stmt.line);
                    }
                    let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                    self.chunk.write(OpCode::OpDefineGlobal(name_idx), stmt.line);
                }
                Ok(())
            }
            StatementKind::If { condition, then_branch, else_branch } => {
                self.compile_expression(condition)?;
                
                let jump_if_false_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJumpIfFalse(0), stmt.line);
                self.chunk.write(OpCode::OpPop, stmt.line);

                self.begin_scope();
                for stmt in then_branch {
                    self.compile_statement(stmt, false)?;
                }
                self.end_scope();

                let jump_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpJump(0), stmt.line);

                let false_target = self.chunk.code.len();
                let offset = false_target - jump_if_false_idx - 1;
                self.chunk.code[jump_if_false_idx] = OpCode::OpJumpIfFalse(offset);
                
                self.chunk.write(OpCode::OpPop, stmt.line);

                if let Some(else_stmts) = else_branch {
                    self.begin_scope();
                    for stmt in else_stmts {
                        self.compile_statement(stmt, false)?;
                    }
                    self.end_scope();
                }

                let end_target = self.chunk.code.len();
                let jump_offset = end_target - jump_idx - 1;
                self.chunk.code[jump_idx] = OpCode::OpJump(jump_offset);

                Ok(())
            }
            StatementKind::ForClassic { init, condition, update, body } => {
                self.begin_scope();
                if let Some(init_stmt) = init {
                    self.compile_statement(init_stmt, false)?;
                }

                let loop_start = self.chunk.code.len();

                let mut exit_jump = None;
                if let Some(cond_expr) = condition {
                    self.compile_expression(cond_expr)?;
                    let jump_idx = self.chunk.code.len();
                    self.chunk.write(OpCode::OpJumpIfFalse(0), stmt.line);
                    self.chunk.write(OpCode::OpPop, stmt.line);
                    exit_jump = Some(jump_idx);
                }

                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }

                if let Some(update_expr) = update {
                    self.compile_expression(update_expr)?;
                    self.chunk.write(OpCode::OpPop, stmt.line);
                }

                let loop_end = self.chunk.code.len();
                let offset = loop_end - loop_start + 1;
                self.chunk.write(OpCode::OpLoop(offset), stmt.line);

                if let Some(idx) = exit_jump {
                    let exit_target = self.chunk.code.len();
                    let offset = exit_target - idx - 1;
                    self.chunk.code[idx] = OpCode::OpJumpIfFalse(offset);
                    self.chunk.write(OpCode::OpPop, stmt.line);
                }
                self.end_scope();

                Ok(())
            }
            StatementKind::FunctionDecl { name, access_modifier: _, return_type: _, params, body } => {
                let func = self.compile_function(&name, &params, &body)?;
                if self.is_repl && is_last {
                    let func_idx = self.chunk.add_constant(BxValue::CompiledFunction(Rc::new(func.clone())));
                    self.chunk.write(OpCode::OpConstant(func_idx), stmt.line);
                }
                let func_idx = self.chunk.add_constant(BxValue::CompiledFunction(Rc::new(func)));
                self.chunk.write(OpCode::OpConstant(func_idx), stmt.line);
                let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                self.chunk.write(OpCode::OpDefineGlobal(name_idx), stmt.line);
                Ok(())
            }
            StatementKind::ForLoop { item, index, collection, body } => {
                self.begin_scope();
                
                self.compile_expression(collection)?;
                let collection_slot = self.locals.len();
                self.locals.push(Local { name: "$collection".to_string(), depth: self.scope_depth });

                let zero_idx = self.chunk.add_constant(BxValue::Number(0.0));
                self.chunk.write(OpCode::OpConstant(zero_idx), stmt.line);
                let cursor_slot = self.locals.len();
                self.locals.push(Local { name: "$cursor".to_string(), depth: self.scope_depth });

                let loop_start = self.chunk.code.len();

                let has_index = index.is_some();
                let iter_next_idx = self.chunk.code.len();
                self.chunk.write(OpCode::OpIterNext(collection_slot, cursor_slot, 0, has_index), stmt.line);

                self.add_local(item.clone());
                if let Some(index_name) = index {
                    self.add_local(index_name.clone());
                }

                for stmt in body {
                    self.compile_statement(stmt, false)?;
                }

                if index.is_some() {
                    self.chunk.write(OpCode::OpPop, stmt.line);
                    self.locals.pop();
                }
                self.chunk.write(OpCode::OpPop, stmt.line);
                self.locals.pop();

                let loop_end = self.chunk.code.len();
                let offset = loop_end - loop_start + 1;
                self.chunk.write(OpCode::OpLoop(offset), stmt.line);

                let exit_target = self.chunk.code.len();
                let offset = exit_target - iter_next_idx - 1;
                self.chunk.code[iter_next_idx] = OpCode::OpIterNext(collection_slot, cursor_slot, offset, has_index);

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
                
                if resolved_path.contains('.') {
                    let class_val = self.load_class_from_path(&resolved_path)?;
                    let class_idx = self.chunk.add_constant(class_val);
                    self.chunk.write(OpCode::OpConstant(class_idx), expr.line);
                } else {
                    let class_idx = self.chunk.add_constant(BxValue::String(resolved_path));
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
                self.chunk.write(OpCode::OpNew(args.len()), expr.line);
                
                // Automatically call init() if args were passed
                if !args.is_empty() {
                    let name_idx = self.chunk.add_constant(BxValue::String("init".to_string()));
                    if has_named {
                        let names_idx = self.chunk.add_constant(BxValue::StringArray(arg_names));
                        self.chunk.write(OpCode::OpInvokeNamed(name_idx, args.len(), names_idx), expr.line);
                    } else {
                        self.chunk.write(OpCode::OpInvoke(name_idx, args.len()), expr.line);
                    }
                }
                
                Ok(())
            }
            ExpressionKind::Literal(lit) => match lit {
                Literal::Number(n) => {
                    let idx = self.chunk.add_constant(BxValue::Number(*n));
                    self.chunk.write(OpCode::OpConstant(idx), expr.line);
                    Ok(())
                }
                Literal::String(parts) => {
                    if parts.is_empty() {
                        let idx = self.chunk.add_constant(BxValue::String("".to_string()));
                        self.chunk.write(OpCode::OpConstant(idx), expr.line);
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
                    let idx = self.chunk.add_constant(BxValue::Boolean(*b));
                    self.chunk.write(OpCode::OpConstant(idx), expr.line);
                    Ok(())
                }
                Literal::Null => {
                    let idx = self.chunk.add_constant(BxValue::Null);
                    self.chunk.write(OpCode::OpConstant(idx), expr.line);
                    Ok(())
                }
                Literal::Array(items) => {
                    for item in items {
                        self.compile_expression(item)?;
                    }
                    self.chunk.write(OpCode::OpArray(items.len()), expr.line);
                    Ok(())
                }
                Literal::Struct(members) => {
                    for (key_expr, val_expr) in members {
                        match &key_expr.kind {
                            ExpressionKind::Identifier(name) => {
                                let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                                self.chunk.write(OpCode::OpConstant(idx), expr.line);
                            }
                            _ => self.compile_expression(key_expr)?,
                        }
                        self.compile_expression(val_expr)?;
                    }
                    self.chunk.write(OpCode::OpStruct(members.len()), expr.line);
                    Ok(())
                }
                Literal::Function { params, body } => {
                    let func = self.compile_function("anonymous", &params, &body)?;
                    let func_idx = self.chunk.add_constant(BxValue::CompiledFunction(Rc::new(func)));
                    self.chunk.write(OpCode::OpConstant(func_idx), expr.line);
                    Ok(())
                }
            },
            ExpressionKind::Binary { left, operator, right } => {
                self.compile_expression(left)?;
                self.compile_expression(right)?;
                match operator.as_str() {
                    "+" => self.chunk.write(OpCode::OpAdd, expr.line),
                    "-" => self.chunk.write(OpCode::OpSubtract, expr.line),
                    "*" => self.chunk.write(OpCode::OpMultiply, expr.line),
                    "/" => self.chunk.write(OpCode::OpDivide, expr.line),
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
            ExpressionKind::Identifier(name) => {
                let lower_name = name.to_lowercase();
                if lower_name == "this" {
                    let idx = self.chunk.add_constant(BxValue::String("this".to_string()));
                    self.chunk.write(OpCode::OpGetPrivate(idx), expr.line);
                } else if lower_name == "variables" {
                    let idx = self.chunk.add_constant(BxValue::String("variables".to_string()));
                    self.chunk.write(OpCode::OpGetPrivate(idx), expr.line);
                } else if let Some(slot) = self.resolve_local(name) {
                    self.chunk.write(OpCode::OpGetLocal(slot), expr.line);
                } else if self.is_class {
                    let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                    self.chunk.write(OpCode::OpGetPrivate(idx), expr.line);
                } else {
                    let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                    self.chunk.write(OpCode::OpGetGlobal(idx), expr.line);
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
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.write(OpCode::OpSetLocal(slot), expr.line);
                        } else if self.is_class {
                            let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetPrivate(name_idx), expr.line);
                        } else {
                            let name_idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetGlobal(name_idx), expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.compile_expression(value)?;
                        let name_idx = self.chunk.add_constant(BxValue::String(member.clone()));
                        self.chunk.write(OpCode::OpSetMember(name_idx), expr.line);
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
                        arg_names.push(name.clone());
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
                        self.chunk.write(OpCode::OpPrintln(args.len()), expr.line);
                        let null_idx = self.chunk.add_constant(BxValue::Null);
                        self.chunk.write(OpCode::OpConstant(null_idx), expr.line);
                        return Ok(());
                    }
                    if lower_name == "print" {
                        for arg in args {
                            self.compile_expression(&arg.value)?;
                        }
                        self.chunk.write(OpCode::OpPrint(args.len()), expr.line);
                        let null_idx = self.chunk.add_constant(BxValue::Null);
                        self.chunk.write(OpCode::OpConstant(null_idx), expr.line);
                        return Ok(());
                    }
                }
                
                if let ExpressionKind::MemberAccess { base: member_base, member } = &base.kind {
                    self.compile_expression(member_base)?;
                    for arg in args {
                        self.compile_expression(&arg.value)?;
                    }
                    let name_idx = self.chunk.add_constant(BxValue::String(member.clone()));
                    if has_named {
                        let names_idx = self.chunk.add_constant(BxValue::StringArray(arg_names));
                        self.chunk.write(OpCode::OpInvokeNamed(name_idx, args.len(), names_idx), expr.line);
                    } else {
                        self.chunk.write(OpCode::OpInvoke(name_idx, args.len()), expr.line);
                    }
                    return Ok(());
                }

                self.compile_expression(base)?;
                for arg in args {
                    self.compile_expression(&arg.value)?;
                }
                if has_named {
                    let names_idx = self.chunk.add_constant(BxValue::StringArray(arg_names));
                    self.chunk.write(OpCode::OpCallNamed(args.len(), names_idx), expr.line);
                } else {
                    self.chunk.write(OpCode::OpCall(args.len()), expr.line);
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
                let name_idx = self.chunk.add_constant(BxValue::String(member.clone()));
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
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.write(OpCode::OpSetLocal(slot), expr.line);
                        } else if self.is_class {
                            let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetPrivate(idx), expr.line);
                        } else {
                            let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetGlobal(idx), expr.line);
                        }
                    }
                    crate::ast::AssignmentTarget::Member { base, member } => {
                        self.compile_expression(base)?;
                        self.chunk.write(OpCode::OpDup, expr.line);
                        let name_idx = self.chunk.add_constant(BxValue::String(member.clone()));
                        self.chunk.write(OpCode::OpMember(name_idx), expr.line);
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        self.chunk.write(OpCode::OpSetMember(name_idx), expr.line);
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
                        if let Some(slot) = self.resolve_local(name) {
                            self.chunk.write(OpCode::OpSetLocal(slot), expr.line);
                        } else if self.is_class {
                            let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetPrivate(idx), expr.line);
                        } else {
                            let idx = self.chunk.add_constant(BxValue::String(name.clone()));
                            self.chunk.write(OpCode::OpSetGlobal(idx), expr.line);
                        }
                        self.chunk.write(OpCode::OpPop, expr.line);
                    }
                    ExpressionKind::MemberAccess { base: member_base, member } => {
                        self.compile_expression(member_base)?;
                        self.chunk.write(OpCode::OpDup, expr.line);
                        let name_idx = self.chunk.add_constant(BxValue::String(member.clone()));
                        self.chunk.write(OpCode::OpMember(name_idx), expr.line);
                        self.chunk.write(OpCode::OpSwap, expr.line);
                        self.chunk.write(OpCode::OpOver, expr.line);
                        if operator == "++" {
                            self.chunk.write(OpCode::OpInc, expr.line);
                        } else {
                            self.chunk.write(OpCode::OpDec, expr.line);
                        }
                        self.chunk.write(OpCode::OpSetMember(name_idx), expr.line);
                        self.chunk.write(OpCode::OpPop, expr.line);
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
                let idx = self.chunk.add_constant(BxValue::String(t.clone()));
                self.chunk.write(OpCode::OpConstant(idx), self.current_line);
                Ok(())
            }
            StringPart::Expression(expr) => self.compile_expression(expr),
        }
    }

    fn compile_function(&mut self, name: &str, params: &[crate::ast::FunctionParam], body: &crate::ast::FunctionBody) -> Result<BxCompiledFunction> {
        let mut sub_compiler = Compiler::new(&self.chunk.filename);
        sub_compiler.chunk.source = self.chunk.source.clone();
        sub_compiler.scope_depth = 1;
        sub_compiler.is_class = self.is_class;
        sub_compiler.imports = self.imports.clone();
        sub_compiler.current_line = self.current_line;

        let mut min_arity = 0;
        for (i, param) in params.iter().enumerate() {
            if param.required {
                min_arity = i + 1;
            }
            sub_compiler.locals.push(Local {
                name: param.name.clone(),
                depth: 1,
            });
        }

        // Emit default value logic at the beginning of the function
        for (i, param) in params.iter().enumerate() {
            if let Some(default_expr) = &param.default_value {
                sub_compiler.chunk.write(OpCode::OpGetLocal(i), self.current_line);
                let null_idx = sub_compiler.chunk.add_constant(BxValue::Null);
                sub_compiler.chunk.write(OpCode::OpConstant(null_idx), self.current_line);
                sub_compiler.chunk.write(OpCode::OpEqual, self.current_line);
                
                let jump_idx = sub_compiler.chunk.code.len();
                sub_compiler.chunk.write(OpCode::OpJumpIfFalse(0), self.current_line);
                
                // True branch: value IS null
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop true
                sub_compiler.compile_expression(default_expr)?;
                sub_compiler.chunk.write(OpCode::OpSetLocal(i), self.current_line);
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop set value
                
                let end_jump_idx = sub_compiler.chunk.code.len();
                sub_compiler.chunk.write(OpCode::OpJump(0), self.current_line);

                // False target: value IS NOT null
                let false_target = sub_compiler.chunk.code.len();
                let offset = false_target - jump_idx - 1;
                sub_compiler.chunk.code[jump_idx] = OpCode::OpJumpIfFalse(offset);
                sub_compiler.chunk.write(OpCode::OpPop, self.current_line); // pop false

                let end_target = sub_compiler.chunk.code.len();
                let end_offset = end_target - end_jump_idx - 1;
                sub_compiler.chunk.code[end_jump_idx] = OpCode::OpJump(end_offset);
            }
        }

        match body {
            FunctionBody::Block(stmts) => {
                for stmt in stmts {
                    sub_compiler.compile_statement(stmt, false)?;
                }
                let null_idx = sub_compiler.chunk.add_constant(BxValue::Null);
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

        Ok(BxCompiledFunction {
            name: name.to_string(),
            arity: params.len(),
            min_arity,
            params: params.iter().map(|p| p.name.clone()).collect(),
            chunk: Rc::new(RefCell::new(sub_compiler.chunk)),
        })
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        for (i, local) in self.locals.iter().enumerate().rev() {
            if local.name.to_lowercase() == name.to_lowercase() {
                return Some(i);
            }
        }
        None
    }

    fn add_local(&mut self, name: String) {
        self.locals.push(Local {
            name,
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

    fn load_class_from_path(&mut self, class_path: &str) -> Result<BxValue> {
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
            if let BxValue::Class(_) = constant {
                return Ok(constant);
            }
        }
        
        bail!("No class declaration found in {}", path.display());
    }

    fn load_interface_from_path(&mut self, iface_path: &str) -> Result<BxValue> {
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
            if let BxValue::Interface(_) = constant {
                return Ok(constant);
            }
        }
        
        bail!("No interface declaration found in {}", path.display());
    }
}
