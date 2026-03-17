use pest::Parser;
use pest_derive::Parser;
use anyhow::{Result, bail, anyhow};
use crate::ast::{Expression, ExpressionKind, Literal, Statement, StatementKind, ClassMember, AssignmentTarget};

#[derive(Parser)]
#[grammar = "parser/boxlang.pest"]
pub struct BxParser;

pub fn parse(source: &str) -> Result<Vec<Statement>> {
    let mut ast = Vec::new();
    let pairs = BxParser::parse(Rule::program, source)?;

    for pair in pairs {
        if pair.as_rule() == Rule::program {
            for inner_pair in pair.into_inner() {
                let line = inner_pair.as_span().start_pos().line_col().0 as u32 as u32;
                match inner_pair.as_rule() {
                    Rule::EOI => break,
                    Rule::import_stmt => {
                        let mut inner = inner_pair.into_inner();
                        let _kw = inner.next().unwrap();
                        let path = inner.next().unwrap().as_str().to_string();
                        let mut alias = None;
                        if let Some(pair) = inner.next() {
                            if pair.as_rule() == Rule::as_keyword {
                                alias = Some(inner.next().unwrap().as_str().to_string());
                            }
                        }
                        ast.push(Statement::new(StatementKind::Import { path, alias }, line));
                    }
                    Rule::statement => {
                        ast.push(parse_statement(inner_pair)?);
                    }
                    _ => {}
                }
            }
        }
    }
    Ok(ast)
}

fn parse_params(pair: pest::iterators::Pair<Rule>) -> Result<Vec<crate::ast::FunctionParam>> {
    let mut params = Vec::new();
    for param_decl in pair.into_inner() {
        let mut required = false;
        let mut type_name = None;
        let mut name = String::new();
        let mut default_value = None;
        
        for inner in param_decl.into_inner() {
            match inner.as_rule() {
                Rule::required_keyword => required = true,
                Rule::type_name => type_name = Some(inner.as_str().to_string()),
                Rule::identifier => name = inner.as_str().to_string(),
                Rule::expression => default_value = Some(parse_expression(inner)?),
                _ => {}
            }
        }
        params.push(crate::ast::FunctionParam { name, type_name, required, default_value });
    }
    Ok(params)
}

fn parse_init(pair: pest::iterators::Pair<Rule>) -> Result<Statement> {
    let inner = pair.into_inner().next().ok_or_else(|| anyhow!("Empty init"))?;
    parse_statement(inner)
}

fn parse_block(pair: pest::iterators::Pair<Rule>) -> Result<Vec<Statement>> {
    let mut stmts = Vec::new();
    for inner in pair.into_inner() {
        stmts.push(parse_statement(inner)?);
    }
    Ok(stmts)
}

fn parse_args(pair: pest::iterators::Pair<Rule>) -> Result<Vec<crate::ast::Argument>> {
    let mut args = Vec::new();
    for arg_pair in pair.into_inner() {
        let mut inner = arg_pair.into_inner();
        let first = inner.next().ok_or_else(|| anyhow!("Empty arg"))?;
        
        match first.as_rule() {
            Rule::identifier => {
                if let Some(value_pair) = inner.next() {
                    // named: identifier = expression
                    let name = Some(first.as_str().to_string());
                    let value = parse_expression(value_pair)?;
                    args.push(crate::ast::Argument { name, value });
                } else {
                    let value = Expression::new(ExpressionKind::Identifier(first.as_str().to_string()), first.as_span().start_pos().line_col().0 as u32);
                    args.push(crate::ast::Argument { name: None, value });
                }
            }
            Rule::expression => {
                let value = parse_expression(first)?;
                args.push(crate::ast::Argument { name: None, value });
            }
            _ => bail!("Unexpected rule in arg: {:?}", first.as_rule()),
        }
    }
    Ok(args)
}

fn parse_variable_decl_core(pair: pest::iterators::Pair<Rule>, line: u32) -> Result<Statement> {
    let mut inner_rules = pair.into_inner();
    let _kw = inner_rules.next().unwrap(); // var_keyword
    let assignment_rule = inner_rules.next().unwrap();
    let mut assignment_inner = assignment_rule.into_inner();
    let target_rule = assignment_inner.next().unwrap();
    let target = parse_target(target_rule)?;
    let value = parse_expression(assignment_inner.next().unwrap())?;
    
    if let AssignmentTarget::Identifier(name) = target {
        Ok(Statement::new(StatementKind::VariableDecl { name, value }, line))
    } else {
        bail!("'var' only supported for simple identifiers");
    }
}

fn parse_attribute(pair: pest::iterators::Pair<Rule>) -> Result<crate::ast::Attribute> {
    let mut inner = pair.into_inner();
    let name = inner.next().unwrap().as_str().to_string();
    let mut args = Vec::new();
    if let Some(args_rule) = inner.next() {
        args = parse_args(args_rule)?;
    }
    Ok(crate::ast::Attribute { name, args })
}

fn parse_statement(pair: pest::iterators::Pair<Rule>) -> Result<Statement> {
    let line = pair.as_span().start_pos().line_col().0 as u32;
    let rule = pair.as_rule();
    match rule {
        Rule::statement => {
            let inner = pair.into_inner().next().unwrap();
            parse_statement(inner)
        }
        Rule::class_decl => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // class_keyword
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut extends = None;
            let mut accessors = false;
            let mut implements = Vec::new();
            let mut members = Vec::new();
            for attr_or_member in inner_rules {
                match attr_or_member.as_rule() {
                    Rule::class_attr => {
                        let attr_pair = attr_or_member.into_inner().next().unwrap();
                        match attr_pair.as_rule() {
                            Rule::extends_attr => {
                                let string_rule = attr_pair.into_inner().next().unwrap();
                                let raw_str = string_rule.as_str().to_string();
                                if raw_str.len() >= 2 && (raw_str.starts_with('"') || raw_str.starts_with('\'')) {
                                    extends = Some(raw_str[1..raw_str.len()-1].to_string());
                                } else {
                                    extends = Some(raw_str);
                                }
                            }
                            Rule::accessors_attr => {
                                let string_rule = attr_pair.into_inner().next().unwrap();
                                let raw_str = string_rule.as_str().to_string();
                                let val = if raw_str.len() >= 2 && (raw_str.starts_with('"') || raw_str.starts_with('\'')) {
                                    raw_str[1..raw_str.len()-1].to_string()
                                } else {
                                    raw_str
                                };
                                accessors = val.to_lowercase() == "true";
                            }
                            Rule::implements_attr => {
                                let string_rule = attr_pair.into_inner().next().unwrap();
                                let raw_str = string_rule.as_str().to_string();
                                let val = if raw_str.len() >= 2 && (raw_str.starts_with('"') || raw_str.starts_with('\'')) {
                                    raw_str[1..raw_str.len()-1].to_string()
                                } else {
                                    raw_str
                                };
                                implements = val.split(',').map(|s| s.trim().to_string()).collect();
                            }
                            _ => bail!("Unexpected class attribute: {:?}", attr_pair.as_rule()),
                        }
                    }
                    Rule::class_member => {
                        let member_inner = attr_or_member.into_inner().next().unwrap();
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
                    _ => bail!("Unexpected rule in class_decl: {:?}", attr_or_member.as_rule()),
                }
            }
            Ok(Statement::new(StatementKind::ClassDecl { name, extends, accessors, implements, members }, line))
        }
        Rule::interface_decl => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // interface_keyword
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut members = Vec::new();
            for member_pair in inner_rules {
                let member_inner = member_pair.into_inner().next().unwrap();
                members.push(parse_statement(member_inner)?);
            }
            Ok(Statement::new(StatementKind::InterfaceDecl { name, members }, line))
        }
        Rule::import_stmt => {
            let mut inner = pair.into_inner();
            let _kw = inner.next().unwrap();
            let path = inner.next().unwrap().as_str().to_string();
            let mut alias = None;
            if let Some(pair) = inner.next() {
                if pair.as_rule() == Rule::as_keyword {
                    alias = Some(inner.next().unwrap().as_str().to_string());
                }
            }
            Ok(Statement::new(StatementKind::Import { path, alias }, line))
        }
        Rule::function_decl => {
            let mut inner_rules = pair.into_inner();
            
            let mut attributes = Vec::new();
            let mut access_modifier = None;
            let mut return_type = None;
            
            let mut current = inner_rules.next().unwrap();
            while current.as_rule() == Rule::attribute {
                attributes.push(parse_attribute(current)?);
                current = inner_rules.next().unwrap();
            }

            if current.as_rule() == Rule::access_modifier {
                access_modifier = Some(current.as_str().to_string());
                current = inner_rules.next().unwrap();
            }
            if current.as_rule() == Rule::type_name {
                return_type = Some(current.as_str().to_string());
                current = inner_rules.next().unwrap();
            }
            
            // next is function_keyword
            let _kw = current;
            
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut params = Vec::new();
            
            let mut next = inner_rules.next();
            if let Some(n) = next.as_ref() {
                if n.as_rule() == Rule::params {
                    params = parse_params(next.take().unwrap())?;
                    next = inner_rules.next();
                }
            }
            
            let body = if let Some(n) = next {
                if n.as_rule() == Rule::block {
                    let body_stmts = parse_block(n)?;
                    crate::ast::FunctionBody::Block(body_stmts)
                } else {
                    crate::ast::FunctionBody::Abstract
                }
            } else {
                crate::ast::FunctionBody::Abstract
            };
            
            Ok(Statement::new(StatementKind::FunctionDecl { 
                name, 
                attributes,
                access_modifier,
                return_type,
                params, 
                body
            }, line))
        }
        Rule::for_loop => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // for_keyword
            let loop_type = inner_rules.next().unwrap();
            let body_rule = inner_rules.next().unwrap();
            let body = parse_block(body_rule)?;

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
                    Ok(Statement::new(StatementKind::ForLoop { item, index, collection, body }, line))
                }
                Rule::for_classic => {
                    let rules = loop_type.into_inner();
                    let mut init = None;
                    let mut condition = None;
                    let mut update = None;
                    
                    for rule in rules {
                        match rule.as_rule() {
                            Rule::init => init = Some(Box::new(parse_init(rule)?)),
                            Rule::condition => condition = Some(parse_expression(rule)?),
                            Rule::update => update = Some(parse_expression(rule)?),
                            _ => {}
                        }
                    }
                    Ok(Statement::new(StatementKind::ForClassic { init, condition, update, body }, line))
                }
                _ => bail!("Unexpected for_loop variant: {:?}", loop_type.as_rule()),
            }
        }
        Rule::if_statement => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // if_keyword
            let condition = parse_expression(inner_rules.next().unwrap())?;
            let then_block_rule = inner_rules.next().unwrap();
            let then_branch = parse_block(then_block_rule)?;
            let mut else_branch = None;

            if let Some(_else_kw) = inner_rules.next() {
                let else_rule = inner_rules.next().unwrap();
                match else_rule.as_rule() {
                    Rule::block => {
                        else_branch = Some(parse_block(else_rule)?);
                    }
                    Rule::if_statement => {
                        else_branch = Some(vec![parse_statement(else_rule)?]);
                    }
                    _ => bail!("Unexpected rule in else branch: {:?}", else_rule.as_rule()),
                }
            }

            Ok(Statement::new(StatementKind::If { condition, then_branch, else_branch }, line))
        }

        Rule::try_catch => {
            let mut inner_rules = pair.into_inner();
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
            Ok(Statement::new(StatementKind::TryCatch { try_branch, catches, finally_branch }, line))
        }
        Rule::return_stmt => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // return_keyword
            let expr = if let Some(p) = inner_rules.next() {
                Some(parse_expression(p)?)
            } else {
                None
            };
            Ok(Statement::new(StatementKind::Return(expr), line))
        }
        Rule::throw_stmt => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // throw_keyword
            let expr = if let Some(p) = inner_rules.next() {
                Some(parse_expression(p)?)
            } else {
                None
            };
            Ok(Statement::new(StatementKind::Throw(expr), line))
        }
        Rule::continue_stmt => {
            Ok(Statement::new(StatementKind::Continue, line))
        }
        Rule::variable_decl => {
            let inner = pair.into_inner().next().unwrap(); // variable_decl_core
            parse_variable_decl_core(inner, line)
        }
        Rule::variable_decl_core => {
            parse_variable_decl_core(pair, line)
        }
        Rule::assignment => {
            let expr = parse_expression(pair)?;
            Ok(Statement::new(StatementKind::Expression(expr), line))
        }
        Rule::expression_stmt => {
            let expr = parse_expression(pair.into_inner().next().unwrap())?;
            Ok(Statement::new(StatementKind::Expression(expr), line))
        }
        _ => bail!("Unexpected statement rule: {:?}", rule),
    }
}

fn parse_expression(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let line = pair.as_span().start_pos().line_col().0 as u32;
    let rule = pair.as_rule();
    match rule {
        Rule::expression | Rule::init | Rule::condition | Rule::update => {
            let inner = pair.into_inner().next().ok_or_else(|| anyhow!("Empty expression"))?;
            parse_expression(inner)
        }
        Rule::conditional_expr => {
            let mut inner = pair.into_inner();
            let binary_pair = inner.next().unwrap();
            let expr = parse_expression(binary_pair)?;
            let second_pair = inner.next();
            if let Some(second) = second_pair {
                let second_expr = parse_expression(second)?;
                if let Some(third) = inner.next() {
                    let third_expr = parse_expression(third)?;
                    Ok(Expression::new(ExpressionKind::Ternary {
                        condition: Box::new(expr),
                        then_expr: Box::new(second_expr),
                        else_expr: Box::new(third_expr),
                    }, line))
                } else {
                    Ok(Expression::new(ExpressionKind::Elvis {
                        left: Box::new(expr),
                        right: Box::new(second_expr),
                    }, line))
                }
            } else {
                Ok(expr)
            }
        }
        Rule::assignment => {
            let mut rules = pair.into_inner();
            let target_rule = rules.next().unwrap();
            let target = parse_target(target_rule)?;
            let value = parse_expression(rules.next().unwrap())?;
            Ok(Expression::new(ExpressionKind::Assignment { target, value: Box::new(value) }, line))
        }
        Rule::binary_expr => {
            let mut rules: Vec<_> = pair.into_inner().collect();
            parse_binary_precedence(&mut rules, 0, line)
        }

        _ => bail!("Unexpected expression rule: {:?}", rule),
    }
}

fn parse_binary_precedence(rules: &mut [pest::iterators::Pair<Rule>], min_precedence: u8, line: u32) -> Result<Expression> {
    // This is a simple implementation of Pratt parsing for a flat list of rules: [primary, op, primary, op, primary]
    // Since we can't easily consume from the slice in recursion without complex logic,
    // we'll use a slightly different approach or just fix the bvm.bxs for now.
    
    // Actually, I'll just implement a simple one that works for the current grammar structure.
    let mut left = parse_primary(rules[0].clone())?;
    let mut i = 1;
    
    while i < rules.len() {
        let op_str = rules[i].as_str();
        let prec = get_precedence(op_str);
        if prec < min_precedence {
            break;
        }
        
        let op = op_str.to_string();
        i += 1;
        
        // Find how far we can go with higher precedence
        let mut j = i;
        while j + 1 < rules.len() && get_precedence(rules[j+1].as_str()) > prec {
            j += 2;
        }
        
        let right = if j == i {
            parse_primary(rules[i].clone())?
        } else {
            parse_binary_precedence(&mut rules[i..j+1], prec + 1, line)?
        };
        
        left = Expression::new(ExpressionKind::Binary {
            left: Box::new(left),
            operator: op,
            right: Box::new(right),
        }, line);
        
        i = j + 1;
    }
    
    Ok(left)
}

fn get_precedence(op: &str) -> u8 {
    match op {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" | "<" | ">" | "<=" | ">=" => 3,
        "+" | "-" | "&" => 4,
        "*" | "/" => 5,
        _ => 0,
    }
}

fn parse_target(pair: pest::iterators::Pair<Rule>) -> Result<AssignmentTarget> {
    let mut inner = pair.into_inner();
    let atom_pair = inner.next().unwrap();
    let mut target_expr = parse_atom(atom_pair)?;

    let accessors: Vec<_> = inner.collect();
    if accessors.is_empty() {
        if let ExpressionKind::Identifier(name) = target_expr.kind {
            return Ok(AssignmentTarget::Identifier(name));
        } else {
            bail!("Invalid assignment target");
        }
    }

    for i in 0..accessors.len()-1 {
        let postfix = &accessors[i];
        let postfix_line = postfix.as_span().start_pos().line_col().0 as u32;
        match postfix.as_rule() {
            Rule::array_access => {
                let index_expr = parse_expression(postfix.clone().into_inner().next().unwrap())?;
                target_expr = Expression::new(ExpressionKind::ArrayAccess {
                    base: Box::new(target_expr),
                    index: Box::new(index_expr),
                }, postfix_line);
            }
            Rule::member_access => {
                let member = postfix.clone().into_inner().next().unwrap().as_str().to_string();
                target_expr = Expression::new(ExpressionKind::MemberAccess {
                    base: Box::new(target_expr),
                    member,
                }, postfix_line);
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
        let postfix_line = postfix.as_span().start_pos().line_col().0 as u32;
        match postfix.as_rule() {
            Rule::function_call_args => {
                let mut args = Vec::new();
                if let Some(args_rule) = postfix.into_inner().next() {
                    args = parse_args(args_rule)?;
                }
                expr = Expression::new(ExpressionKind::FunctionCall {
                    base: Box::new(expr),
                    args,
                }, postfix_line);
            }
            Rule::array_access => {
                let index_expr = parse_expression(postfix.into_inner().next().unwrap())?;
                expr = Expression::new(ExpressionKind::ArrayAccess {
                    base: Box::new(expr),
                    index: Box::new(index_expr),
                }, postfix_line);
            }
            Rule::member_access => {
                let member = postfix.into_inner().next().unwrap().as_str().to_string();
                expr = Expression::new(ExpressionKind::MemberAccess {
                    base: Box::new(expr),
                    member,
                }, postfix_line);
            }
            Rule::safe_member_access => {
                let member = postfix.into_inner().next().unwrap().as_str().to_string();
                expr = Expression::new(ExpressionKind::SafeMemberAccess {
                    base: Box::new(expr),
                    member,
                }, postfix_line);
            }
            Rule::postfix_op => {
                let operator = postfix.as_str().to_string();
                expr = Expression::new(ExpressionKind::Postfix {
                    base: Box::new(expr),
                    operator,
                }, postfix_line);
            }
            _ => bail!("Unexpected postfix rule: {:?}", postfix.as_rule()),
        }
    }
    Ok(expr)
}

fn parse_atom(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let line = pair.as_span().start_pos().line_col().0 as u32;
    let rule = pair.as_rule();
    match rule {
        Rule::atom => {
            let inner = pair.into_inner().next().unwrap();
            parse_atom(inner)
        }
        Rule::unary_not => {
            let inner = pair.into_inner().next().unwrap();
            let expr = parse_primary(inner)?;
            Ok(Expression::new(ExpressionKind::UnaryNot(Box::new(expr)), line))
        }
        Rule::prefix_op => {
            let operator = if pair.as_str().starts_with("++") { "++" } else { "--" }.to_string();
            let mut inner_rules = pair.into_inner();
            let target = parse_target(inner_rules.next().unwrap())?;
            Ok(Expression::new(ExpressionKind::Prefix { operator, target }, line))
        }
        Rule::new_expression => {
            let mut inner_rules = pair.into_inner();
            let _kw = inner_rules.next().unwrap(); // new_keyword
            let class_path = inner_rules.next().unwrap().as_str().to_string();
            let mut args = Vec::new();
            if let Some(args_rule) = inner_rules.next() {
                args = parse_args(args_rule)?;
            }
            Ok(Expression::new(ExpressionKind::New { class_path, args }, line))
        }
        Rule::literal => {
            let lit = pair.into_inner().next().unwrap();
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
                    Ok(Expression::new(ExpressionKind::Literal(Literal::String(parts)), line))
                }
                Rule::number => {
                    let n = lit.as_str().parse::<f64>()?;
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Number(n)), line))
                }
                Rule::boolean => {
                    let b = lit.as_str().trim() == "true";
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Boolean(b)), line))
                }
                Rule::null_lit => {
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Null), line))
                }
                Rule::array_literal => {
                    let mut items = Vec::new();
                    for expr in lit.into_inner() {
                        items.push(parse_expression(expr)?);
                    }
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Array(items)), line))
                }
                Rule::struct_literal => {
                    let mut members = Vec::new();
                    for member_pair in lit.into_inner() {
                        let mut member_inner = member_pair.into_inner();
                        let key_pair = member_inner.next().unwrap().into_inner().next().unwrap();
                        let key_expr = match key_pair.as_rule() {
                            Rule::identifier => Expression::new(ExpressionKind::Identifier(key_pair.as_str().to_string()), line),
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
                                Expression::new(ExpressionKind::Literal(Literal::String(parts)), line)
                            }
                            Rule::number => {
                                let n = key_pair.as_str().parse::<f64>()?;
                                Expression::new(ExpressionKind::Literal(Literal::Number(n)), line)
                            }
                            _ => bail!("Invalid struct key: {:?}", key_pair.as_rule()),
                        };
                        let val_expr = parse_expression(member_inner.next().unwrap())?;
                        members.push((key_expr, val_expr));
                    }
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Struct(members)), line))
                }
                Rule::anonymous_function => {
                    let mut inner = lit.into_inner();
                    let first = inner.next().unwrap();
                    let (params, body_rule) = if first.as_rule() == Rule::lambda_params {
                        let params = {
                            let mut param_inner = first.clone().into_inner();
                            if let Some(param_rule) = param_inner.next() {
                                match param_rule.as_rule() {
                                    Rule::params => parse_params(param_rule)?,
                                    Rule::identifier => vec![crate::ast::FunctionParam {
                                        name: param_rule.as_str().to_string(),
                                        type_name: None,
                                        required: false,
                                        default_value: None,
                                    }],
                                    _ => vec![],
                                }
                            } else {
                                vec![]
                            }
                        };
                        // Operands like => are literals, not rules, so they don't appear in into_inner()
                        (params, inner.next().unwrap())
                    } else {
                        // first is function_keyword, next is params or block
                        let mut next = inner.next().unwrap();
                        let params = if next.as_rule() == Rule::params {
                            let p = parse_params(next)?;
                            next = inner.next().unwrap();
                            p
                        } else {
                            vec![]
                        };
                        (params, next)
                    };
                    
                    let body = if body_rule.as_rule() == Rule::block {
                        let stmts = parse_block(body_rule)?;
                        crate::ast::FunctionBody::Block(stmts)
                    } else {
                        crate::ast::FunctionBody::Expression(Box::new(parse_expression(body_rule)?))
                    };
                    
                    Ok(Expression::new(ExpressionKind::Literal(Literal::Function { params, body }), line))
                }
                _ => bail!("Unexpected literal rule: {:?}", lit.as_rule()),
            }
        }
        Rule::identifier => {
            Ok(Expression::new(ExpressionKind::Identifier(pair.as_str().to_string()), line))
        }
        Rule::expression => parse_expression(pair),
        _ => bail!("Unexpected atom rule: {:?}", rule),
    }
}
