use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use anyhow::{Result, bail};
use crate::ast::{Expression, Literal, Statement};
use crate::env::Environment;
use crate::types::{BxFunction, BxValue};

pub struct Evaluator {
    pub env: Rc<RefCell<Environment>>,
}

impl Evaluator {
    pub fn new() -> Self {
        Evaluator {
            env: Environment::new(),
        }
    }

    pub fn with_env(env: Rc<RefCell<Environment>>) -> Self {
        Evaluator { env }
    }

    pub fn eval_program(&mut self, statements: &[Statement]) -> Result<BxValue> {
        let mut result = BxValue::Null;
        for stmt in statements {
            result = self.eval_statement(stmt)?;
        }
        Ok(result)
    }

    pub fn eval_statement(&mut self, stmt: &Statement) -> Result<BxValue> {
        match stmt {
            Statement::Expression(expr) => self.eval_expression(expr),
            Statement::FunctionDecl { name, params, body } => {
                let func = BxValue::Function(BxFunction {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                });
                self.env.borrow_mut().define(name.clone(), func.clone());
                Ok(func)
            }
            Statement::If { condition, then_branch, else_branch } => {
                let cond_val = self.eval_expression(condition)?;
                if is_truthy(&cond_val) {
                    self.eval_block(then_branch)
                } else if let Some(else_branch) = else_branch {
                    self.eval_block(else_branch)
                } else {
                    Ok(BxValue::Null)
                }
            }
            Statement::ForClassic { init, condition, update, body } => {
                if let Some(init_expr) = init {
                    self.eval_expression(init_expr)?;
                }

                while let Some(cond_expr) = condition {
                    let cond_val = self.eval_expression(cond_expr)?;
                    if !is_truthy(&cond_val) {
                        break;
                    }
                    self.eval_block(body)?;
                    if let Some(update_expr) = update {
                        self.eval_expression(update_expr)?;
                    }
                }
                
                // If there's no condition, it's an infinite loop in some languages, 
                // but for this POC let's just run it once or handle it.
                // BoxLang/Java allows for(;;).
                if condition.is_none() {
                    loop {
                        self.eval_block(body)?;
                        if let Some(update_expr) = update {
                            self.eval_expression(update_expr)?;
                        }
                    }
                }

                Ok(BxValue::Null)
            }
            Statement::ForLoop { item, index, collection, body } => {
                let coll_val = self.eval_expression(collection)?;
                
                match coll_val {
                    BxValue::Array(items) => {
                        for (i, val) in items.iter().enumerate() {
                            // BoxLang arrays are 1-indexed, so we use 1-based index
                            let idx = i as f64 + 1.0;
                            
                            // Define variables in a sub-scope for the loop body
                            let loop_env = Environment::new_with_parent(Rc::clone(&self.env));
                            loop_env.borrow_mut().define(item.clone(), val.clone());
                            if let Some(index_name) = index {
                                loop_env.borrow_mut().define(index_name.clone(), BxValue::Number(idx));
                            }
                            
                            let mut loop_evaluator = Evaluator::with_env(loop_env);
                            loop_evaluator.eval_block(body)?;
                        }
                    }
                    _ => bail!("Iteration only supported for arrays currently"),
                }
                Ok(BxValue::Null)
            }
        }
    }

    pub fn eval_block(&mut self, statements: &[Statement]) -> Result<BxValue> {
        let mut result = BxValue::Null;
        for stmt in statements {
            result = self.eval_statement(stmt)?;
        }
        Ok(result)
    }

    pub fn eval_expression(&mut self, expr: &Expression) -> Result<BxValue> {
        match expr {
            Expression::Literal(lit) => match lit {
                Literal::String(parts) => {
                    let mut result = String::new();
                    for part in parts {
                        match part {
                            crate::ast::StringPart::Text(t) => result.push_str(t),
                            crate::ast::StringPart::Expression(expr) => {
                                let val = self.eval_expression(expr)?;
                                result.push_str(&val.to_string());
                            }
                        }
                    }
                    Ok(BxValue::String(result))
                }
                Literal::Number(n) => Ok(BxValue::Number(*n)),
                Literal::Boolean(b) => Ok(BxValue::Boolean(*b)),
                Literal::Null => Ok(BxValue::Null),
                Literal::Array(items) => {
                    let mut eval_items = Vec::new();
                    for item in items {
                        eval_items.push(self.eval_expression(item)?);
                    }
                    Ok(BxValue::Array(eval_items))
                }
                Literal::Struct(members) => {
                    let mut eval_struct = HashMap::new();
                    for (key_expr, val_expr) in members {
                        let key = match key_expr {
                            Expression::Identifier(name) => name.to_lowercase(),
                            _ => self.eval_expression(key_expr)?.to_string().to_lowercase(),
                        };
                        let val = self.eval_expression(val_expr)?;
                        eval_struct.insert(key, val);
                    }
                    Ok(BxValue::Struct(eval_struct))
                }
            },
            Expression::Identifier(name) => {
                if let Some(val) = self.env.borrow().get(name) {
                    Ok(val)
                } else {
                    // Implicit null for undefined in some loosely typed contexts, or error.
                    // Let's return Null for undefined just like CFML/BoxLang sometimes does.
                    Ok(BxValue::Null)
                }
            }
            Expression::Assignment { target, value } => {
                let val = self.eval_expression(value)?;
                self.env.borrow_mut().assign(target, val.clone())
                    .map_err(|e| anyhow::anyhow!("Assignment error: {}", e))?;
                Ok(val)
            }
            Expression::Binary { left, operator, right } => {
                let left_val = self.eval_expression(left)?;
                let right_val = self.eval_expression(right)?;
                self.eval_binary(&left_val, operator, &right_val)
            }
            Expression::FunctionCall { base, args } => {
                let mut evaluated_args = Vec::new();
                for arg in args {
                    evaluated_args.push(self.eval_expression(arg)?);
                }

                // Check for BIFs directly in this POC
                if let Expression::Identifier(name) = base.as_ref() {
                    if name.to_lowercase() == "println" || name.to_lowercase() == "echo" {
                        let out = evaluated_args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ");
                        println!("{}", out);
                        return Ok(BxValue::Null);
                    }
                    if name.to_lowercase() == "print" {
                        let out = evaluated_args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ");
                        print!("{}", out);
                        return Ok(BxValue::Null);
                    }
                }

                let func_val = self.eval_expression(base)?;
                match func_val {
                    BxValue::Function(func) => {
                        let call_env = Environment::new_with_parent(Rc::clone(&self.env));
                        for (i, param) in func.params.iter().enumerate() {
                            if i < evaluated_args.len() {
                                call_env.borrow_mut().define(param.clone(), evaluated_args[i].clone());
                            } else {
                                call_env.borrow_mut().define(param.clone(), BxValue::Null);
                            }
                        }
                        let mut call_eval = Evaluator::with_env(call_env);
                        call_eval.eval_block(&func.body)
                    }
                    _ => bail!("Value is not callable"),
                }
            }
            Expression::ArrayAccess { base, index } => {
                let base_val = self.eval_expression(base)?;
                let index_val = self.eval_expression(index)?;
                
                match (base_val, index_val) {
                    (BxValue::Array(arr), BxValue::Number(n)) => {
                        let idx = n as usize;
                        if idx < 1 || idx > arr.len() {
                            bail!("Array index out of bounds: {}", idx);
                        }
                        Ok(arr[idx - 1].clone())
                    }
                    (BxValue::Struct(s), key_val) => {
                        let key = key_val.to_string().to_lowercase();
                        if let Some(val) = s.get(&key) {
                            Ok(val.clone())
                        } else {
                            Ok(BxValue::Null)
                        }
                    }
                    _ => bail!("Invalid access: base must be array or struct"),
                }
            }
            Expression::MemberAccess { base, member } => {
                let base_val = self.eval_expression(base)?;
                match base_val {
                    BxValue::Struct(s) => {
                        let key = member.to_lowercase();
                        if let Some(val) = s.get(&key) {
                            Ok(val.clone())
                        } else {
                            Ok(BxValue::Null)
                        }
                    }
                    _ => bail!("Member access only supported on structs"),
                }
            }
        }
    }

    fn eval_binary(&self, left: &BxValue, operator: &str, right: &BxValue) -> Result<BxValue> {
        match operator {
            "+" => {
                match (left, right) {
                    (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Number(l + r)),
                    (BxValue::String(l), BxValue::String(r)) => Ok(BxValue::String(format!("{}{}", l, r))),
                    _ => bail!("Unsupported operands for +: {:?} + {:?}", left, right),
                }
            }
            "-" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Number(l - r)),
                _ => bail!("Unsupported operands for -"),
            },
            "*" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Number(l * r)),
                _ => bail!("Unsupported operands for *"),
            },
            "/" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => {
                    if *r == 0.0 { bail!("Division by zero"); }
                    Ok(BxValue::Number(l / r))
                },
                _ => bail!("Unsupported operands for /"),
            },
            "==" => Ok(BxValue::Boolean(left == right)),
            "!=" => Ok(BxValue::Boolean(left != right)),
            "<" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Boolean(l < r)),
                _ => bail!("Unsupported operands for <"),
            },
            "<=" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Boolean(l <= r)),
                _ => bail!("Unsupported operands for <="),
            },
            ">" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Boolean(l > r)),
                _ => bail!("Unsupported operands for >"),
            },
            ">=" => match (left, right) {
                (BxValue::Number(l), BxValue::Number(r)) => Ok(BxValue::Boolean(l >= r)),
                _ => bail!("Unsupported operands for >="),
            },
            "&" => {
                Ok(BxValue::String(format!("{}{}", left.to_string(), right.to_string())))
            }
            _ => bail!("Unsupported operator: {}", operator),
        }
    }
}

fn is_truthy(val: &BxValue) -> bool {
    match val {
        BxValue::Boolean(b) => *b,
        BxValue::Null => false,
        BxValue::Number(n) => *n != 0.0,
        BxValue::String(s) => !s.is_empty() && s.to_lowercase() != "false",
        _ => true,
    }
}
