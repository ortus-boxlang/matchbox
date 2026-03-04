use pest::Parser;
use pest_derive::Parser;
use anyhow::{Result, bail, anyhow};
use crate::ast::{Expression, Literal, Statement, ClassMember, AssignmentTarget};

#[derive(Parser)]
#[grammar = "parser/boxlang.pest"]
pub struct BxParser;

pub fn parse(source: &str) -> Result<Vec<Statement>> {
    let mut ast = Vec::new();
    let pairs = BxParser::parse(Rule::program, source)?;

    for pair in pairs {
        if pair.as_rule() == Rule::program {
            for inner_pair in pair.into_inner() {
                if inner_pair.as_rule() == Rule::EOI {
                    break;
                }
                ast.push(parse_statement(inner_pair)?);
            }
        }
    }
    Ok(ast)
}

fn parse_params(pair: pest::iterators::Pair<Rule>) -> Vec<String> {
    pair.into_inner().map(|p| p.as_str().to_string()).collect()
}

fn parse_block(pair: pest::iterators::Pair<Rule>) -> Result<Vec<Statement>> {
    let mut stmts = Vec::new();
    for inner in pair.into_inner() {
        stmts.push(parse_statement(inner)?);
    }
    Ok(stmts)
}

fn parse_statement(pair: pest::iterators::Pair<Rule>) -> Result<Statement> {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::class_decl => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // class_keyword
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut members = Vec::new();
            for member_pair in inner_rules {
                let member_inner = member_pair.into_inner().next().unwrap();
                match member_inner.as_rule() {
                    Rule::property => {
                        let mut prop_inner = member_inner.into_inner();
                        let _kw = prop_inner.next().unwrap(); // property_keyword
                        let prop_name = prop_inner.next().unwrap().as_str().to_string();
                        members.push(ClassMember::Property(prop_name));
                    }
                    Rule::statement => {
                        members.push(ClassMember::Statement(parse_statement(member_inner)?));
                    }
                    _ => bail!("Unexpected class member rule: {:?}", member_inner.as_rule()),
                }
            }
            Ok(Statement::ClassDecl { name, members })
        }
        Rule::function_decl => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // function_keyword
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut params = Vec::new();
            
            let mut next = inner_rules.next().unwrap();
            if next.as_rule() == Rule::params {
                params = parse_params(next);
                next = inner_rules.next().unwrap();
            }
            
            let mut body_stmts = Vec::new();
            if next.as_rule() == Rule::block {
                for stmt_pair in next.into_inner() {
                    body_stmts.push(parse_statement(stmt_pair)?);
                }
            }
            
            Ok(Statement::FunctionDecl { 
                name, 
                params, 
                body: crate::ast::FunctionBody::Block(body_stmts) 
            })
        }
        Rule::for_loop => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // for_keyword
            let loop_type = inner_rules.next().unwrap();
            let body_rule = inner_rules.next().unwrap();
            let mut body = Vec::new();
            for stmt_rule in body_rule.into_inner() {
                body.push(parse_statement(stmt_rule)?);
            }

            match loop_type.as_rule() {
                Rule::for_in => {
                    let mut rules = loop_type.into_inner();
                    let item = rules.next().unwrap().as_str().to_string();
                    let mut index = None;
                    let next_rule = rules.next().unwrap();
                    let collection = if next_rule.as_rule() == Rule::identifier {
                        index = Some(next_rule.as_str().to_string());
                        let _in = rules.next().unwrap(); // in_keyword
                        parse_expression(rules.next().unwrap())?
                    } else {
                        // next_rule IS in_keyword
                        parse_expression(rules.next().unwrap())?
                    };
                    Ok(Statement::ForLoop { item, index, collection, body })
                }
                Rule::for_classic => {
                    let rules = loop_type.into_inner();
                    let mut init = None;
                    let mut condition = None;
                    let mut update = None;
                    
                    for rule in rules {
                        match rule.as_rule() {
                            Rule::init => init = Some(parse_expression(rule)?),
                            Rule::condition => condition = Some(parse_expression(rule)?),
                            Rule::update => update = Some(parse_expression(rule)?),
                            _ => {}
                        }
                    }
                    Ok(Statement::ForClassic { init, condition, update, body })
                }
                _ => bail!("Unexpected for_loop variant: {:?}", loop_type.as_rule()),
            }
        }
        Rule::if_statement => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // if_keyword
            let condition = parse_expression(inner_rules.next().unwrap())?;
            let mut then_branch = Vec::new();
            let mut else_branch = None;
            
            let then_block_rule = inner_rules.next().unwrap();
            for stmt_rule in then_block_rule.into_inner() {
                then_branch.push(parse_statement(stmt_rule)?);
            }

            if let Some(_next) = inner_rules.next() {
                // Could be else_keyword
                let else_block_rule = inner_rules.next().unwrap();
                let mut e_branch = Vec::new();
                for stmt_rule in else_block_rule.into_inner() {
                    e_branch.push(parse_statement(stmt_rule)?);
                }
                else_branch = Some(e_branch);
            }
            
            Ok(Statement::If { condition, then_branch, else_branch })
        }
        Rule::try_catch => {
            let mut inner_rules = inner.into_inner();
            let _try_kw = inner_rules.next().unwrap();
            let try_branch = parse_block(inner_rules.next().unwrap())?;
            
            let mut catches = Vec::new();
            let mut finally_branch = None;
            
            for rule in inner_rules {
                match rule.as_rule() {
                    Rule::catch_block => {
                        let mut catch_inner = rule.into_inner();
                        let _catch_kw = catch_inner.next().unwrap();
                        let exception_var = catch_inner.next().unwrap().as_str().to_string();
                        let body = parse_block(catch_inner.next().unwrap())?;
                        catches.push(crate::ast::CatchBlock { exception_var, body });
                    }
                    Rule::finally_block => {
                        let mut finally_inner = rule.into_inner();
                        let _finally_kw = finally_inner.next().unwrap();
                        finally_branch = Some(parse_block(finally_inner.next().unwrap())?);
                    }
                    _ => {}
                }
            }
            Ok(Statement::TryCatch { try_branch, catches, finally_branch })
        }
        Rule::return_stmt => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // return_keyword
            let expr = if let Some(pair) = inner_rules.next() {
                Some(parse_expression(pair)?)
            } else {
                None
            };
            Ok(Statement::Return(expr))
        }
        Rule::throw_stmt => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // throw_keyword
            let expr = if let Some(pair) = inner_rules.next() {
                Some(parse_expression(pair)?)
            } else {
                None
            };
            Ok(Statement::Throw(expr))
        }
        Rule::variable_decl => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // var_keyword
            let assignment_rule = inner_rules.next().unwrap();
            let mut assignment_inner = assignment_rule.into_inner();
            let target_rule = assignment_inner.next().unwrap();
            let target = parse_target(target_rule)?;
            let value = parse_expression(assignment_inner.next().unwrap())?;
            
            // VariableDecl in AST currently only supports simple name
            if let AssignmentTarget::Identifier(name) = target {
                Ok(Statement::VariableDecl { name, value })
            } else {
                bail!("'var' only supported for simple identifiers");
            }
        }
        Rule::expression_stmt => {
            let expr = parse_expression(inner.into_inner().next().unwrap())?;
            Ok(Statement::Expression(expr))
        }
        _ => bail!("Unexpected statement rule: {:?}", inner.as_rule()),
    }
}

fn parse_expression(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let rule = pair.as_rule();
    match rule {
        Rule::expression | Rule::init | Rule::condition | Rule::update => {
            let inner = pair.into_inner().next().ok_or_else(|| anyhow!("Empty expression"))?;
            parse_expression(inner)
        }
        Rule::assignment => {
            let mut rules = pair.into_inner();
            let target_rule = rules.next().unwrap();
            let target = parse_target(target_rule)?;
            let value = parse_expression(rules.next().unwrap())?;
            Ok(Expression::Assignment { target, value: Box::new(value) })
        }
        Rule::binary_expr => {
            let mut rules = pair.into_inner();
            let mut left = parse_primary(rules.next().unwrap())?;
            
            while let Some(op) = rules.next() {
                let operator = op.as_str().to_string();
                let right = parse_primary(rules.next().unwrap())?;
                left = Expression::Binary {
                    left: Box::new(left),
                    operator,
                    right: Box::new(right),
                };
            }
            Ok(left)
        }
        _ => bail!("Unexpected expression rule: {:?}", rule),
    }
}

fn parse_target(pair: pest::iterators::Pair<Rule>) -> Result<AssignmentTarget> {
    let mut inner = pair.into_inner();
    let atom_pair = inner.next().unwrap();
    let mut target_expr = parse_atom(atom_pair)?;

    let accessors: Vec<_> = inner.collect();
    if accessors.is_empty() {
        if let Expression::Identifier(name) = target_expr {
            return Ok(AssignmentTarget::Identifier(name));
        } else {
            bail!("Invalid assignment target");
        }
    }

    for i in 0..accessors.len()-1 {
        let postfix = &accessors[i];
        match postfix.as_rule() {
            Rule::array_access => {
                let index_expr = parse_expression(postfix.clone().into_inner().next().unwrap())?;
                target_expr = Expression::ArrayAccess {
                    base: Box::new(target_expr),
                    index: Box::new(index_expr),
                };
            }
            Rule::member_access => {
                let member = postfix.clone().into_inner().next().unwrap().as_str().to_string();
                target_expr = Expression::MemberAccess {
                    base: Box::new(target_expr),
                    member,
                };
            }
            _ => bail!("Unexpected target postfix rule: {:?}", postfix.as_rule()),
        }
    }

    let last = accessors.last().unwrap();
    match last.as_rule() {
        Rule::array_access => {
            let index_expr = parse_expression(last.clone().into_inner().next().unwrap())?;
            Ok(AssignmentTarget::Index {
                base: Box::new(target_expr),
                index: Box::new(index_expr),
            })
        }
        Rule::member_access => {
            let member = last.clone().into_inner().next().unwrap().as_str().to_string();
            Ok(AssignmentTarget::Member {
                base: Box::new(target_expr),
                member,
            })
        }
        _ => bail!("Invalid assignment target postfix"),
    }
}

fn parse_primary(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let mut inner = pair.into_inner();
    let atom_pair = inner.next().unwrap();
    let mut expr = parse_atom(atom_pair)?;

    for postfix in inner {
        match postfix.as_rule() {
            Rule::function_call_args => {
                let mut args = Vec::new();
                if let Some(args_rule) = postfix.into_inner().next() {
                    for arg in args_rule.into_inner() {
                        args.push(parse_expression(arg)?);
                    }
                }
                expr = Expression::FunctionCall {
                    base: Box::new(expr),
                    args,
                };
            }
            Rule::array_access => {
                let index_expr = parse_expression(postfix.into_inner().next().unwrap())?;
                expr = Expression::ArrayAccess {
                    base: Box::new(expr),
                    index: Box::new(index_expr),
                };
            }
            Rule::member_access => {
                let member = postfix.into_inner().next().unwrap().as_str().to_string();
                expr = Expression::MemberAccess {
                    base: Box::new(expr),
                    member,
                };
            }
            _ => bail!("Unexpected postfix rule: {:?}", postfix.as_rule()),
        }
    }
    Ok(expr)
}

fn parse_atom(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::new_expression => {
            let mut inner_rules = inner.into_inner();
            let _kw = inner_rules.next().unwrap(); // new_keyword
            let class_name = inner_rules.next().unwrap().as_str().to_string();
            let mut args = Vec::new();
            if let Some(args_rule) = inner_rules.next() {
                for arg in args_rule.into_inner() {
                    args.push(parse_expression(arg)?);
                }
            }
            Ok(Expression::New { class_name, args })
        }
        Rule::literal => {
            let lit = inner.into_inner().next().unwrap();
            match lit.as_rule() {
                Rule::string => {
                    let mut parts = Vec::new();
                    for part in lit.into_inner() {
                        match part.as_rule() {
                            Rule::string_text_double | Rule::string_text_single => {
                                parts.push(crate::ast::StringPart::Text(part.as_str().to_string()));
                            }
                            Rule::escaped_hash => {
                                parts.push(crate::ast::StringPart::Text("#".to_string()));
                            }
                            Rule::interpolation => {
                                let expr = parse_expression(part.into_inner().next().unwrap())?;
                                parts.push(crate::ast::StringPart::Expression(expr));
                            }
                            _ => bail!("Unexpected string part rule: {:?}", part.as_rule()),
                        }
                    }
                    Ok(Expression::Literal(Literal::String(parts)))
                }
                Rule::number => {
                    let n = lit.as_str().parse::<f64>()?;
                    Ok(Expression::Literal(Literal::Number(n)))
                }
                Rule::boolean => {
                    let b = lit.as_str().trim() == "true";
                    Ok(Expression::Literal(Literal::Boolean(b)))
                }
                Rule::null_lit => {
                    Ok(Expression::Literal(Literal::Null))
                }
                Rule::array_literal => {
                    let mut items = Vec::new();
                    for expr in lit.into_inner() {
                        items.push(parse_expression(expr)?);
                    }
                    Ok(Expression::Literal(Literal::Array(items)))
                }
                Rule::struct_literal => {
                    let mut members = Vec::new();
                    for member_pair in lit.into_inner() {
                        let mut member_inner = member_pair.into_inner();
                        let key_pair = member_inner.next().unwrap().into_inner().next().unwrap();
                        let key_expr = match key_pair.as_rule() {
                            Rule::identifier => Expression::Identifier(key_pair.as_str().to_string()),
                            Rule::string => {
                                // Specialized string parsing
                                let mut parts = Vec::new();
                                for part in key_pair.into_inner() {
                                    match part.as_rule() {
                                        Rule::string_text_double | Rule::string_text_single => {
                                            parts.push(crate::ast::StringPart::Text(part.as_str().to_string()));
                                        }
                                        Rule::escaped_hash => {
                                            parts.push(crate::ast::StringPart::Text("#".to_string()));
                                        }
                                        Rule::interpolation => {
                                            let expr = parse_expression(part.into_inner().next().unwrap())?;
                                            parts.push(crate::ast::StringPart::Expression(expr));
                                        }
                                        _ => bail!("Unexpected string part in struct key: {:?}", part.as_rule()),
                                    }
                                }
                                Expression::Literal(Literal::String(parts))
                            }
                            Rule::number => {
                                let n = key_pair.as_str().parse::<f64>()?;
                                Expression::Literal(Literal::Number(n))
                            }
                            _ => bail!("Invalid struct key: {:?}", key_pair.as_rule()),
                        };
                        let val_expr = parse_expression(member_inner.next().unwrap())?;
                        members.push((key_expr, val_expr));
                    }
                    Ok(Expression::Literal(Literal::Struct(members)))
                }
                Rule::anonymous_function => {
                    let mut inner = lit.into_inner();
                    let first = inner.next().unwrap();
                    let (params, body_rule) = if first.as_rule() == Rule::lambda_params {
                        let params = {
                            let mut param_inner = first.clone().into_inner();
                            let param_rule = param_inner.next().unwrap();
                            match param_rule.as_rule() {
                                Rule::params => parse_params(param_rule),
                                Rule::identifier => vec![param_rule.as_str().to_string()],
                                _ => vec![],
                            }
                        };
                        // Operands like => are literals, not rules, so they don't appear in into_inner()
                        (params, inner.next().unwrap())
                    } else {
                        // first is function_keyword, next is params or block
                        let next = inner.next().unwrap();
                        if next.as_rule() == Rule::params {
                            let params = parse_params(next);
                            (params, inner.next().unwrap())
                        } else {
                            (vec![], next)
                        }
                    };
                    
                    let body = if body_rule.as_rule() == Rule::block {
                        let mut stmts = Vec::new();
                        for stmt_pair in body_rule.into_inner() {
                            stmts.push(parse_statement(stmt_pair)?);
                        }
                        crate::ast::FunctionBody::Block(stmts)
                    } else {
                        crate::ast::FunctionBody::Expression(Box::new(parse_expression(body_rule)?))
                    };
                    
                    Ok(Expression::Literal(Literal::Function { params, body }))
                }
                _ => bail!("Unexpected literal rule: {:?}", lit.as_rule()),
            }
        }
        Rule::identifier => {
            Ok(Expression::Identifier(inner.as_str().to_string()))
        }
        Rule::expression => parse_expression(inner),
        _ => bail!("Unexpected atom rule: {:?}", inner.as_rule()),
    }
}
