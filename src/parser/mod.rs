use pest::Parser;
use pest_derive::Parser;
use anyhow::{Result, bail};
use crate::ast::{Expression, Literal, Statement};

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

fn parse_statement(pair: pest::iterators::Pair<Rule>) -> Result<Statement> {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::function_decl => {
            let mut inner_rules = inner.into_inner();
            let name = inner_rules.next().unwrap().as_str().to_string();
            let mut params = Vec::new();
            let mut body = Vec::new();
            
            for rule in inner_rules {
                match rule.as_rule() {
                    Rule::params => {
                        for param in rule.into_inner() {
                            params.push(param.as_str().to_string());
                        }
                    }
                    Rule::statement => {
                        body.push(parse_statement(rule)?);
                    }
                    _ => {}
                }
            }
            Ok(Statement::FunctionDecl { name, params, body })
        }
        Rule::for_loop => {
            let mut inner_rules = inner.into_inner();
            let item = inner_rules.next().unwrap().as_str().to_string();
            let collection = parse_expression(inner_rules.next().unwrap())?;
            let mut body = Vec::new();
            
            for rule in inner_rules {
                if rule.as_rule() == Rule::statement {
                    body.push(parse_statement(rule)?);
                }
            }
            Ok(Statement::ForLoop { item, collection, body })
        }
        Rule::if_statement => {
            let mut inner_rules = inner.into_inner();
            let condition = parse_expression(inner_rules.next().unwrap())?;
            let mut then_branch = Vec::new();
            let mut else_branch = None;
            
            let then_block_rule = inner_rules.next().unwrap();
            for stmt_rule in then_block_rule.into_inner() {
                then_branch.push(parse_statement(stmt_rule)?);
            }

            if let Some(else_block_rule) = inner_rules.next() {
                let mut e_branch = Vec::new();
                for stmt_rule in else_block_rule.into_inner() {
                    e_branch.push(parse_statement(stmt_rule)?);
                }
                else_branch = Some(e_branch);
            }
            
            Ok(Statement::If { condition, then_branch, else_branch })
        }
        Rule::expression_stmt => {
            let expr = parse_expression(inner.into_inner().next().unwrap())?;
            Ok(Statement::Expression(expr))
        }
        _ => bail!("Unexpected statement rule: {:?}", inner.as_rule()),
    }
}

fn parse_expression(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::assignment => {
            let mut rules = inner.into_inner();
            let target = rules.next().unwrap().as_str().to_string();
            let value = parse_expression(rules.next().unwrap())?;
            Ok(Expression::Assignment { target, value: Box::new(value) })
        }
        Rule::binary_expr => {
            let mut rules = inner.into_inner();
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
        _ => bail!("Unexpected expression rule: {:?}", inner.as_rule()),
    }
}

fn parse_primary(pair: pest::iterators::Pair<Rule>) -> Result<Expression> {
    let inner = pair.into_inner().next().unwrap();
    match inner.as_rule() {
        Rule::function_call => {
            let mut rules = inner.into_inner();
            let name = rules.next().unwrap().as_str().to_string();
            let mut args = Vec::new();
            if let Some(args_rule) = rules.next() {
                for arg in args_rule.into_inner() {
                    args.push(parse_expression(arg)?);
                }
            }
            Ok(Expression::FunctionCall { name, args })
        }
        Rule::literal => {
            let lit = inner.into_inner().next().unwrap();
            match lit.as_rule() {
                Rule::string => {
                    let s = lit.into_inner().next().unwrap().as_str().to_string();
                    Ok(Expression::Literal(Literal::String(s)))
                }
                Rule::number => {
                    let n = lit.as_str().parse::<f64>()?;
                    Ok(Expression::Literal(Literal::Number(n)))
                }
                Rule::boolean => {
                    let b = lit.as_str() == "true";
                    Ok(Expression::Literal(Literal::Boolean(b)))
                }
                Rule::null_lit => {
                    Ok(Expression::Literal(Literal::Null))
                }
                _ => bail!("Unexpected literal rule: {:?}", lit.as_rule()),
            }
        }
        Rule::identifier => {
            Ok(Expression::Identifier(inner.as_str().to_string()))
        }
        Rule::expression => parse_expression(inner),
        _ => bail!("Unexpected primary rule: {:?}", inner.as_rule()),
    }
}
