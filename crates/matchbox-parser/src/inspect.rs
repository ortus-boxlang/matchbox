//! Public AST data, independent of Rust enum layouts. BoxLang-style names/fields
//! are used where the compiler AST retains them; compiler-only forms are namespaced.
use crate::{
    ast::*,
    parser,
    tokenizer::{Span, TriviaKind, lex},
};
use serde_json::{Value, json};

pub fn inspect(
    source: &str,
    filename: Option<&str>,
    recover: bool,
    include_tokens: bool,
) -> anyhow::Result<Value> {
    let (statements, issues) = if recover {
        parser::parse_recover(source, filename)
    } else {
        (parser::parse(source, filename)?, Vec::new())
    };
    inspect_parsed(source, &statements, &issues, recover, include_tokens)
}

/// Export an existing authoritative parse without parsing the source again.
pub fn inspect_parsed(source: &str, statements: &[Statement], issues: &[parser::ParseError], recover: bool, include_tokens: bool) -> anyhow::Result<Value> {
    let export = Export::new(source);
    let lexed = lex(source);
    let mut root = export.node(
        "BoxScript",
        Some(Span::at(source, 0, source.len())),
        Value::from_iter([("statements", export.statements(statements))]),
    );
    root["comments"] = json!(
        lexed
            .trivia()
            .iter()
            .filter(|trivia| trivia.kind != TriviaKind::Whitespace)
            .map(|trivia| {
                export.node(
                    if trivia.kind == TriviaKind::LineComment {
                        "BoxSingleLineComment"
                    } else {
                        "BoxMultiLineComment"
                    },
                    Some(trivia.span),
                    json!({"commentText": lexed.text(trivia.span)}),
                )
            })
            .collect::<Vec<_>>()
    );
    if recover {
        root["issues"] = json!(issues.iter().map(|issue| json!({"message": issue.message, "filename": issue.filename, "position": export.position(issue.span), "span": {"start":issue.span.start,"end":issue.span.end}})).collect::<Vec<_>>());
    }
    if include_tokens {
        root["tokens"] = json!(lexed.tokens().iter().map(|token| json!({"kind":token.kind.to_string(), "text":lexed.text(token.span), "position":export.position(token.span), "span":{"start":token.span.start,"end":token.span.end}})).collect::<Vec<_>>());
        root["trivia"] = json!(lexed.trivia().iter().map(|trivia| json!({"kind":match trivia.kind {TriviaKind::Whitespace=>"whitespace", TriviaKind::LineComment=>"lineComment", TriviaKind::BlockComment=>"blockComment"}, "text":lexed.text(trivia.span), "position":export.position(trivia.span), "span":{"start":trivia.span.start,"end":trivia.span.end}})).collect::<Vec<_>>());
    }
    Ok(root)
}

/// Independent flat token/comment/recovery data for a native snapshot. No AST export.
pub fn lexical_parsed(source: &str, issues: &[parser::ParseError]) -> anyhow::Result<Value> {
    let export = Export::new(source);
    let lexed = lex(source);
    Ok(json!({
        "tokens": lexed.tokens().iter().map(|token| json!({"kind":token.kind.to_string(),"text":lexed.text(token.span),"position":export.position(token.span),"span":{"start":token.span.start,"end":token.span.end}})).collect::<Vec<_>>(),
        "comments": lexed.trivia().iter().filter(|t|t.kind!=TriviaKind::Whitespace).map(|t|export.node(if t.kind==TriviaKind::LineComment{"BoxSingleLineComment"}else{"BoxMultiLineComment"},Some(t.span),json!({"commentText":lexed.text(t.span)}))).collect::<Vec<_>>(),
        "issues": issues.iter().map(|i|json!({"message":i.message,"filename":i.filename,"position":export.position(i.span),"span":{"start":i.span.start,"end":i.span.end}})).collect::<Vec<_>>()
    }))
}

struct Export<'a> {
    source: &'a str,
    lines: Vec<usize>,
}
impl<'a> Export<'a> {
    fn new(source: &'a str) -> Self {
        let mut lines = vec![0];
        lines.extend(source.match_indices('\n').map(|(index, _)| index + 1));
        Self { source, lines }
    }
    fn point(&self, offset: usize) -> Value {
        let line = self.lines.partition_point(|start| *start <= offset) - 1;
        // ponytail: per-line UTF-16 scans; cache column offsets if minified inputs dominate.
        json!({"line": line + 1, "column": self.source[self.lines[line]..offset].encode_utf16().count()})
    }
    fn position(&self, span: Span) -> Value {
        json!({"start":self.point(span.start), "end":self.point(span.end)})
    }
    fn node(&self, kind: &str, span: Option<Span>, mut fields: Value) -> Value {
        fields["ASTType"] = json!(kind);
        let package = if kind.starts_with("MatchBox") {
            "matchbox.parser"
        } else if matches!(kind, "BoxScript" | "BoxClass" | "BoxInterface") {
            "ortus.boxlang.compiler.ast"
        } else if kind.ends_with("Comment") {
            "ortus.boxlang.compiler.ast.comment"
        } else if matches!(
            kind,
            "BoxExpressionStatement"
                | "BoxFunctionDeclaration"
                | "BoxReturn"
                | "BoxThrow"
                | "BoxRethrow"
                | "BoxBreak"
                | "BoxContinue"
                | "BoxIfElse"
                | "BoxWhile"
                | "BoxDo"
                | "BoxForIn"
                | "BoxForIndex"
                | "BoxImport"
                | "BoxStatementBlock"
                | "BoxTry"
                | "BoxAssert"
                | "BoxInclude"
                | "BoxParam"
                | "BoxSwitch"
                | "BoxBufferOutput"
                | "BoxComponent"
                | "BoxArgumentDeclaration"
                | "BoxAnnotation"
                | "BoxReturnType"
                | "BoxProperty"
        ) {
            "ortus.boxlang.compiler.ast.statement"
        } else {
            "ortus.boxlang.compiler.ast.expression"
        };
        fields["ASTPackage"] = json!(package);
        fields["sourceText"] = span
            .map(|span| json!(&self.source[span.start..span.end]))
            .unwrap_or(Value::Null);
        fields["comments"] = json!([]);
        if let Some(span) = span {
            fields["position"] = self.position(span);
            fields["span"] = json!({"start":span.start,"end":span.end});
        }
        fields
    }
    fn statements(&self, body: &[Statement]) -> Value {
        body.iter().map(|stmt| self.statement(stmt)).collect()
    }
    fn optional(&self, expr: &Option<Expression>) -> Value {
        expr.as_ref()
            .map(|expr| self.expression(expr))
            .unwrap_or(Value::Null)
    }
    fn block(&self, body: &[Statement]) -> Value {
        let span = body
            .first()
            .and_then(|first| first.span)
            .zip(body.last().and_then(|last| last.span))
            .map(|(mut first, last)| {
                first.end = last.end;
                first
            });
        self.node(
            "BoxStatementBlock",
            span,
            Value::from_iter([("statements", self.statements(body))]),
        )
    }
    fn params(&self, params: &[FunctionParam]) -> Value {
        json!(params.iter().map(|param| self.node("BoxArgumentDeclaration", None, json!({"name":param.name,"type":param.type_name,"required":param.required,"value":self.optional(&param.default_value),"annotations":[],"documentation":[]}))).collect::<Vec<_>>())
    }
    fn body(&self, body: &FunctionBody) -> Value {
        match body {
            FunctionBody::Block(stmts) | FunctionBody::Component(stmts) => self.statements(stmts),
            FunctionBody::Expression(expr) => Value::Array(vec![self.node(
                "BoxReturn",
                expr.span,
                Value::from_iter([("expression", self.expression(expr))]),
            )]),
            FunctionBody::Abstract => Value::Null,
        }
    }
    fn args(&self, args: &[Argument]) -> Value {
        json!(args.iter().map(|arg| self.node("BoxArgument", arg.value.span, json!({"name":arg.name.as_ref().map(|name| self.node("BoxIdentifier",None,json!({"name":name}))),"value":self.expression(&arg.value)}))).collect::<Vec<_>>())
    }
    fn attributes(&self, attributes: &[Attribute]) -> Value {
        json!(
            attributes
                .iter()
                .map(|attr| self.node(
                    "BoxAnnotation",
                    None,
                    json!({"key":attr.name,"value":self.args(&attr.args)})
                ))
                .collect::<Vec<_>>()
        )
    }
    fn statement(&self, stmt: &Statement) -> Value {
        let (kind, fields) = match &stmt.kind {
            StatementKind::Expression(expr) => (
                "BoxExpressionStatement",
                json!({"expression":self.expression(expr)}),
            ),
            StatementKind::VariableDecl { name, value } => {
                let assignment = self.node("BoxAssignment",stmt.span,json!({"left":self.node("BoxIdentifier",None,json!({"name":name})),"right":self.expression(value),"op":self.enum_value("BoxAssignment","Equal"),"modifiers":[self.enum_value("BoxAssignment","Var")]}));
                ("BoxExpressionStatement", json!({"expression":assignment}))
            }
            StatementKind::FunctionDecl {
                name,
                attributes,
                modifiers,
                return_type,
                params,
                body,
            } => (
                "BoxFunctionDeclaration",
                Value::from_iter([
                    ("name", json!(name)), ("args", self.params(params)), ("body", self.body(body)),
                    ("type", return_type.as_ref().map(|name| self.return_type(name)).unwrap_or(Value::Null)),
                    ("accessModifier", modifiers.access.as_ref().map(|name| self.enum_value("BoxFunctionDeclaration", &capitalize(name))).unwrap_or(Value::Null)),
                    ("modifiers", [("Static", modifiers.is_static), ("Abstract", modifiers.is_abstract), ("Final", modifiers.is_final)].iter().filter(|(_, enabled)| *enabled).map(|(name, _)| self.enum_value("BoxFunctionDeclaration", name)).collect()),
                    ("annotations", self.attributes(attributes)), ("documentation", json!([])),
                ]),
            ),
            StatementKind::ClassDecl {
                name,
                attributes,
                modifiers,
                extends,
                accessors,
                implements,
                members,
            } => (
                "BoxClass",
                Value::from_iter([
                    ("name", json!(name)), ("extends", json!(extends)), ("accessors", json!(accessors)), ("implements", json!(implements)),
                    ("annotations", self.attributes(attributes)), ("modifiers", json!({"abstract":modifiers.is_abstract,"final":modifiers.is_final})),
                    ("body", members.iter().filter_map(|member| if let ClassMember::Statement(stmt) = member { Some(self.statement(stmt)) } else { None }).collect()),
                    ("properties", members.iter().filter_map(|member| if let ClassMember::Property(property) = member { Some(self.node("BoxProperty", Some(property.span), Value::from_iter([
                        ("name", json!(property.name)), ("annotations", property.annotations.iter().map(|annotation| self.node("BoxAnnotation", Some(annotation.span), Value::from_iter([
                            ("key", self.node("BoxFQN", Some(annotation.key_span), json!({"value":annotation.key}))), ("value", self.optional(&annotation.value)),
                        ]))).collect()), ("documentation", json!([])),
                    ]))) } else { None }).collect()),
                ]),
            ),
            StatementKind::InterfaceDecl { name, members } => (
                "BoxInterface",
                Value::from_iter([("name", json!(name)), ("body", self.statements(members))]),
            ),
            StatementKind::Import { path, alias } => (
                "BoxImport",
                json!({"expression":self.node("BoxFQN",None,json!({"value":path})),"alias":alias.as_ref().map(|name|self.node("BoxIdentifier",None,json!({"name":name})))}),
            ),
            StatementKind::Return(expr) => ("BoxReturn", json!({"expression":self.optional(expr)})),
            StatementKind::Throw(expr) => ("BoxThrow", json!({"expression":self.optional(expr)})),
            StatementKind::Rethrow => ("BoxRethrow", json!({})),
            StatementKind::Continue(label) => ("BoxContinue", json!({"label":label})),
            StatementKind::Break(label) => ("BoxBreak", json!({"label":label})),
            StatementKind::Labeled { label, body } => ("BoxLabel", json!({"label":label,"body":self.statements(body)})),
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => (
                "BoxIfElse",
                Value::from_iter([
                    ("condition", self.expression(condition)),
                    ("thenBody", self.block(then_branch)),
                    ("elseBody", else_branch.as_ref().map(|body| self.block(body)).unwrap_or(Value::Null)),
                ]),
            ),
            StatementKind::WhileLoop { condition, body } => (
                "BoxWhile",
                Value::from_iter([("condition", self.expression(condition)), ("body", self.block(body))]),
            ),
            StatementKind::DoWhile { body, condition } => (
                "BoxDo",
                Value::from_iter([("condition", self.expression(condition)), ("body", self.block(body))]),
            ),
            StatementKind::ForLoop {
                item,
                index,
                collection,
                body,
            } => (
                "BoxForIn",
                Value::from_iter([
                    ("variable", self.node("BoxIdentifier", None, json!({"name":item}))), ("secondVariable", json!(index)),
                    ("expression", self.expression(collection)), ("body", self.block(body)),
                ]),
            ),
            StatementKind::ForClassic {
                init,
                condition,
                update,
                body,
            } => (
                "BoxForIndex",
                Value::from_iter([
                    ("initializer", init.as_ref().map(|stmt| self.statement(stmt)).unwrap_or(Value::Null)),
                    ("condition", self.optional(condition)), ("step", self.optional(update)), ("body", self.block(body)),
                ]),
            ),
            StatementKind::TryCatch {
                try_branch,
                catches,
                finally_branch,
            } => (
                "BoxTry",
                Value::from_iter([
                    ("tryBody", self.statements(try_branch)),
                    ("catches", catches.iter().map(|catch| Value::from_iter([("exception", json!(catch.exception_var)), ("body", self.statements(&catch.body))])).collect()),
                    ("finallyBody", finally_branch.as_ref().map(|body| self.statements(body)).unwrap_or(Value::Null)),
                ]),
            ),
            StatementKind::Switch {
                value,
                cases,
                default_case,
            } => (
                "BoxSwitch",
                Value::from_iter([
                    ("condition", self.expression(value)),
                    ("cases", cases.iter().map(|case| Value::from_iter([("condition", self.expression(&case.value)), ("body", self.statements(&case.body))])).collect()),
                    ("defaultBody", default_case.as_ref().map(|body| self.statements(body)).unwrap_or(Value::Null)),
                ]),
            ),
            StatementKind::Assert { condition, message } => (
                "BoxAssert",
                json!({"expression":self.expression(condition),"message":self.optional(message)}),
            ),
            StatementKind::Param { name, default } => (
                "BoxParam",
                json!({"name":name,"default":self.optional(default)}),
            ),
            StatementKind::Include(expr) => {
                ("BoxInclude", json!({"expression":self.expression(expr)}))
            }
            StatementKind::BufferOutput(expr) => (
                "BoxBufferOutput",
                json!({"expression":self.expression(expr)}),
            ),
            StatementKind::Not(expr) => (
                "MatchBoxNotStatement",
                json!({"expression":self.expression(expr)}),
            ),
            StatementKind::Destructure {
                kind,
                source,
                bindings,
                rest,
            } => (
                "MatchBoxDestructure",
                json!({"kind":match kind{DestructureKind::Array=>"array",DestructureKind::Object=>"object"},"source":self.expression(source),"bindings":bindings,"rest":rest}),
            ),
            StatementKind::Component {
                name,
                attributes,
                body,
            } => (
                "BoxComponent",
                Value::from_iter([
                    ("name", json!(name)),
                    ("attributes", attributes.iter().map(|(key, value)| Value::from_iter([("key", self.expression(key)), ("value", self.expression(value))])).collect()),
                    ("body", body.as_ref().map(|body| self.statements(body)).unwrap_or(Value::Null)),
                ]),
            ),
        };
        let mut node = self.node(kind, stmt.span, fields);
        if stmt.incomplete { node["incomplete"] = json!(true); }
        if let Some(span) = stmt.body_span { node["bodySpan"] = json!({"start":span.start,"end":span.end}); }
        node
    }
    fn enum_value(&self, kind: &str, name: &str) -> Value {
        let package = if matches!(kind, "BoxFunctionDeclaration" | "BoxReturnType") {
            "ortus.boxlang.compiler.ast.statement"
        } else {
            "ortus.boxlang.compiler.ast.expression"
        };
        json!({"ASTType":kind,"ASTPackage":package,"sourceText":name})
    }
    fn return_type(&self, name: &str) -> Value {
        let known = matches!(
            name.to_ascii_lowercase().as_str(),
            "numeric"
                | "string"
                | "boolean"
                | "component"
                | "interface"
                | "any"
                | "array"
                | "struct"
                | "query"
                | "void"
        );
        let kind = if known {
            capitalize(name)
        } else {
            "Fqn".into()
        };
        self.node("BoxReturnType",None,json!({"type":self.enum_value("BoxReturnType",&kind),"fqn":if known {Value::Null} else {json!(name)}}))
    }
    fn binary_operator(&self, operator: &str) -> (&'static str, String) {
        let (kind, name) = match operator.to_ascii_lowercase().as_str() {
            "==" => ("BoxComparisonOperation", "Equal"),
            "!=" => ("BoxComparisonOperation", "NotEqual"),
            "===" => ("BoxComparisonOperation", "TEqual"),
            "!==" => ("BoxComparisonOperation", "TNotEqual"),
            "<" => ("BoxComparisonOperation", "LessThan"),
            "<=" => ("BoxComparisonOperation", "LessThanEquals"),
            ">" => ("BoxComparisonOperation", "GreaterThan"),
            ">=" => ("BoxComparisonOperation", "GreaterThanEquals"),
            "+" => ("BoxBinaryOperation", "Plus"),
            "-" => ("BoxBinaryOperation", "Minus"),
            "*" => ("BoxBinaryOperation", "Star"),
            "/" => ("BoxBinaryOperation", "Slash"),
            "%" => ("BoxBinaryOperation", "Mod"),
            "^" => ("BoxBinaryOperation", "Power"),
            "\\" => ("BoxBinaryOperation", "Backslash"),
            "&&" => ("BoxBinaryOperation", "And"),
            "||" => ("BoxBinaryOperation", "Or"),
            "xor" => ("BoxBinaryOperation", "Xor"),
            "eqv" => ("BoxBinaryOperation", "Equivalence"),
            "contains" => ("BoxBinaryOperation", "Contains"),
            "not contains" => ("BoxBinaryOperation", "NotContains"),
            "castas" => ("BoxBinaryOperation", "CastAs"),
            "instanceof" => ("BoxBinaryOperation", "InstanceOf"),
            ".." => ("BoxBinaryOperation", "Range"),
            ">.." => ("BoxBinaryOperation", "RangeLeftExclusive"),
            "..<" => ("BoxBinaryOperation", "RangeRightExclusive"),
            ">..<" => ("BoxBinaryOperation", "RangeFullExclusive"),
            "b&" => ("BoxBinaryOperation", "BitwiseAnd"),
            "b|" => ("BoxBinaryOperation", "BitwiseOr"),
            "b^" => ("BoxBinaryOperation", "BitwiseXor"),
            "b<<" => ("BoxBinaryOperation", "BitwiseSignedLeftShift"),
            "b>>" => ("BoxBinaryOperation", "BitwiseSignedRightShift"),
            "b>>>" => ("BoxBinaryOperation", "BitwiseUnsignedRightShift"),
            _ => return ("MatchBoxBinaryOperation", operator.into()),
        };
        (kind, name.into())
    }
    fn target(&self, target: &AssignmentTarget) -> Value {
        match target {
            AssignmentTarget::Identifier(name)=>self.node("BoxIdentifier",None,json!({"name":name})),
            AssignmentTarget::Member{base,member}=>self.node("BoxDotAccess",None,json!({"context":self.expression(base),"access":self.node("BoxIdentifier",None,json!({"name":member})),"safe":false})),
            AssignmentTarget::Index{base,index}=>self.node("BoxArrayAccess",None,json!({"context":self.expression(base),"access":self.expression(index),"safe":false})),
        }
    }
    fn expression(&self, expr: &Expression) -> Value {
        let mut node = self.expression_data(expr);
        if expr.incomplete { node["incomplete"] = json!(true); }
        node
    }
    fn expression_data(&self, expr: &Expression) -> Value {
        let (kind, fields) = match &expr.kind {
            ExpressionKind::Identifier(name) => ("BoxIdentifier", json!({"name":name})),
            ExpressionKind::Literal(literal) => return self.literal(literal, expr.span),
            ExpressionKind::Assignment { target, value } => (
                "BoxAssignment",
                json!({"left":self.target(target),"right":self.expression(value),"op":self.enum_value("BoxAssignment","Equal"),"modifiers":[]}),
            ),
            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                let (kind, name) = self.binary_operator(operator);
                (
                    kind,
                    json!({"left":self.expression(left),"operator":self.enum_value(kind,&name),"right":self.expression(right)}),
                )
            }
            ExpressionKind::UnaryNot(value) | ExpressionKind::UnaryBitwiseNot(value) => (
                "BoxUnaryOperation",
                json!({"expr":self.expression(value),"operator":self.enum_value("BoxUnaryOperation",if matches!(expr.kind,ExpressionKind::UnaryNot(_)){"Not"}else{"BitwiseComplement"})}),
            ),
            ExpressionKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => (
                "BoxTernaryOperation",
                json!({"condition":self.expression(condition),"whenTrue":self.expression(then_expr),"whenFalse":self.expression(else_expr)}),
            ),
            ExpressionKind::Elvis { left, right } => (
                "BoxBinaryOperation",
                json!({"left":self.expression(left),"operator":self.enum_value("BoxBinaryOperation","Elvis"),"right":self.expression(right)}),
            ),
            ExpressionKind::FunctionCall { base, args } => match &base.kind {
                ExpressionKind::Identifier(name) => (
                    "BoxFunctionInvocation",
                    json!({"name":name,"arguments":self.args(args)}),
                ),
                ExpressionKind::MemberAccess {
                    base: object,
                    member,
                }
                | ExpressionKind::SafeMemberAccess {
                    base: object,
                    member,
                } => (
                    "BoxMethodInvocation",
                    json!({"obj":self.expression(object),"name":self.node("BoxIdentifier",None,json!({"name":member})),"arguments":self.args(args),"safe":matches!(base.kind,ExpressionKind::SafeMemberAccess{..})}),
                ),
                _ => (
                    "BoxExpressionInvocation",
                    json!({"expr":self.expression(base),"arguments":self.args(args)}),
                ),
            },
            ExpressionKind::ArrayAccess { base, index } => (
                "BoxArrayAccess",
                json!({"context":self.expression(base),"access":self.expression(index),"safe":false}),
            ),
            ExpressionKind::MemberAccess { base, member }
            | ExpressionKind::SafeMemberAccess { base, member } => (
                "BoxDotAccess",
                json!({"context":self.expression(base),"access":self.node("BoxIdentifier",None,json!({"name":member})),"safe":matches!(expr.kind,ExpressionKind::SafeMemberAccess{..})}),
            ),
            ExpressionKind::New { class_path, args } => (
                "BoxNew",
                json!({"expression":self.node("BoxIdentifier",None,json!({"name":class_path})),"arguments":self.args(args)}),
            ),
            ExpressionKind::Prefix { operator, target } => (
                "BoxUnaryOperation",
                json!({"expr":self.target(target),"operator":operator,"prefix":true}),
            ),
            ExpressionKind::Postfix { base, operator } => (
                "BoxUnaryOperation",
                json!({"expr":self.expression(base),"operator":operator,"prefix":false}),
            ),
            ExpressionKind::Spread(value) => (
                "MatchBoxSpread",
                json!({"expression":self.expression(value)}),
            ),
        };
        self.node(kind, expr.span, fields)
    }
    fn literal(&self, literal: &Literal, span: Option<Span>) -> Value {
        let (kind, fields) = match literal {
            Literal::Null => ("BoxNull", json!({})),
            Literal::Boolean(value) => ("BoxBooleanLiteral", json!({"value":value})),
            Literal::Number(value) => (
                if value.fract() == 0.0 {
                    "BoxIntegerLiteral"
                } else {
                    "BoxDecimalLiteral"
                },
                json!({"value":value.to_string()}),
            ),
            Literal::Array(values) => (
                "BoxArrayLiteral",
                json!({"values":values.iter().map(|value|self.expression(value)).collect::<Vec<_>>() }),
            ),
            Literal::Struct(entries) => (
                "BoxStructLiteral",
                json!({"values":entries.iter().flat_map(|(key,value)|[self.expression(key),self.expression(value)]).collect::<Vec<_>>() }),
            ),
            Literal::String(parts) => {
                if parts.iter().all(|part| matches!(part, StringPart::Text(_))) {
                    (
                        "BoxStringLiteral",
                        json!({"value":parts.iter().map(|part|if let StringPart::Text(text)=part{text.as_str()}else{""}).collect::<String>()}),
                    )
                } else {
                    (
                        "BoxStringInterpolation",
                        json!({"values":parts.iter().map(|part|match part{StringPart::Text(text)=>self.node("BoxStringLiteral",None,json!({"value":text})),StringPart::Expression(expr)=>self.expression(expr)}).collect::<Vec<_>>() }),
                    )
                }
            }
            Literal::Function {
                params,
                body,
                is_lambda,
            } => (
                if *is_lambda {
                    "BoxLambda"
                } else {
                    "BoxClosure"
                },
                Value::from_iter([("args", self.params(params)), ("body", self.body(body))]),
            ),
        };
        self.node(kind, span, fields)
    }
}

fn capitalize(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| {
            first.to_uppercase().collect::<String>() + &chars.as_str().to_ascii_lowercase()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recovery_preserves_function_scope_after_an_invalid_statement() {
        let source = "function edit() { var used=1; var broken=; return used; }";
        assert!(inspect(source, None, false, false).is_err());
        let root = inspect(source, None, true, false).unwrap();
        assert_eq!(root["issues"].as_array().unwrap().len(), 1);
        assert_eq!(root["statements"].as_array().unwrap().len(), 1);
        assert_eq!(root["statements"][0]["name"], "edit");
        assert_eq!(root["statements"][0]["body"].as_array().unwrap().len(), 2);
        assert_eq!(root["statements"][0]["body"][1]["expression"]["name"], "used");
    }

    #[test]
    fn recovery_retains_unclosed_nested_declarations_without_hoisting_locals() {
        let source = "class Editor { function edit() { var used=1; return used;";
        assert!(inspect(source, None, false, false).is_err());
        let root = inspect(source, None, true, false).unwrap();
        assert_eq!(root["issues"].as_array().unwrap().len(), 2);
        assert_eq!(root["statements"].as_array().unwrap().len(), 1);
        assert_eq!(root["statements"][0]["name"], "Editor");
        let function = &root["statements"][0]["body"][0];
        assert_eq!(function["name"], "edit");
        assert_eq!(function["body"].as_array().unwrap().len(), 2);
        assert_eq!(function["sourceText"], "function edit() { var used=1; return used;");
    }

    #[test]
    fn incomplete_scope_spans_include_eof_trivia() {
        let source = "class Open { function closed() { var hidden=1; }\n  ";
        let root = inspect(source, None, true, false).unwrap();
        let class = &root["statements"][0];
        assert_eq!(class["incomplete"], true);
        assert_eq!(class["span"]["end"], source.len());
        assert_eq!(class["sourceText"], source);
        assert!(class["body"][0].get("incomplete").is_none());
        assert!(class["body"][0]["span"]["end"].as_u64().unwrap() < source.len() as u64);
    }

    #[test]
    fn doubled_hashes_remain_literal_text() {
        let root = inspect("var text='##x##';", None, false, false).unwrap();
        assert_eq!(root["statements"][0]["expression"]["right"]["ASTType"], "BoxStringLiteral");
        assert_eq!(root["statements"][0]["expression"]["right"]["value"], "#x#");
    }

    #[test]
    fn declaration_body_spans_exclude_header_default_braces() {
        let source = "class { function run(value={a:1}) { return value; } function empty(value={}) {} }";
        let root = inspect(source, None, false, true).unwrap();
        let class = &root["statements"][0];
        assert_eq!(class["bodySpan"]["start"], source.find('{').unwrap());
        assert_eq!(class["bodySpan"]["end"], source.len());
        for (function, expected) in class["body"].as_array().unwrap().iter().zip(["{ return value; }", "{}"]) {
            let start = function["bodySpan"]["start"].as_u64().unwrap() as usize;
            let end = function["bodySpan"]["end"].as_u64().unwrap() as usize;
            assert_eq!(&source[start..end], expected);
        }
        let unfinished = "function edit() { // inside\n  ";
        let recovered = inspect(unfinished, None, true, true).unwrap();
        assert_eq!(recovered["statements"][0]["bodySpan"]["end"], unfinished.len());
    }

    #[test]
    fn property_annotations_and_source_spans_are_retained() {
        let source = "class { property String label; property name='id' type='numeric' default=1; function run(){return 1;} }";
        let root = inspect(source, None, false, true).unwrap();
        let class = &root["statements"][0];
        assert_eq!(class["body"].as_array().unwrap().len(), 1);
        let properties = class["properties"].as_array().unwrap();
        assert_eq!(properties.len(), 2);
        assert_eq!(properties[0]["name"], "label");
        assert_eq!(properties[0]["sourceText"], "property String label;");
        assert_eq!(properties[0]["annotations"][0]["key"]["value"], "String");
        assert_eq!(properties[1]["name"], "id");
        assert_eq!(properties[1]["sourceText"], "property name='id' type='numeric' default=1;");
        assert_eq!(properties[1]["annotations"][0]["sourceText"], "name='id'");
        assert_eq!(properties[1]["annotations"][0]["key"]["sourceText"], "name");
        assert_eq!(properties[1]["annotations"][0]["value"]["value"], "id");
        assert_eq!(properties[1]["annotations"][2]["value"]["value"], "1");
        assert_eq!(properties[1]["span"]["start"], source.find("property name").unwrap());
    }

    #[test]
    fn positioned_tree_tokens_and_recovery() {
        let source =
            "// comment\nname = \"😀\"; other = 1 + 2 + 3;\nfunction add(a,b) { return a+b; }";
        let root = inspect(source, None, false, true).unwrap();
        assert_eq!(root["statements"][2]["name"], "add");
        assert_eq!(
            root["statements"][2]["body"][0]["expression"]["left"]["name"],
            "a"
        );
        assert_eq!(root["statements"][1]["position"]["start"]["column"], 13);
        assert_eq!(
            root["statements"][1]["expression"]["right"]["left"]["sourceText"],
            "1 + 2"
        );
        assert_eq!(root["comments"][0]["sourceText"], "// comment");
        assert_eq!(
            root["statements"][2]["body"][0]["expression"]["operator"]["sourceText"],
            "Plus"
        );
        let interpolated =
            inspect("prefix=1; text=\"value #name + 1#\";", None, false, false).unwrap();
        assert_eq!(
            interpolated["statements"][1]["expression"]["right"]["values"][1]["left"]["sourceText"],
            "name"
        );
        assert!(root["tokens"].as_array().unwrap().len() > 10);
        assert!(inspect("function broken() {", None, false, false).is_err());
        assert!(inspect("x = '😀", None, false, false).is_err());
        assert!(inspect("/* missing", None, false, false).is_err());
        let recovered = inspect(
            "var good=1; var bad=; var next=2; function broken() {",
            Some("edit.bxs"),
            true,
            true,
        )
        .unwrap();
        assert_eq!(recovered["statements"].as_array().unwrap().len(), 3);
        assert_eq!(recovered["issues"].as_array().unwrap().len(), 2);
        assert_eq!(recovered["issues"][0]["filename"], "edit.bxs");
        assert_eq!(
            inspect("", None, true, true).unwrap()["statements"],
            json!([])
        );
    }
}
