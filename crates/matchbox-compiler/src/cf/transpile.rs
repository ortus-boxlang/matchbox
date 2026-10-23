//! CF -> BoxLang rename pass.
//!
//! Ported from BoxLang's `CFTranspilerVisitor` (the static maps at the top of
//! that file). After a CF file is parsed, this walks the whole tree and renames
//! CF-only names to their BoxLang equivalents, so the compiler sees plain
//! BoxLang.

use crate::ast::*;
use std::collections::HashSet;

/// CF built-in functions whose BoxLang name differs. Only renames whose target
/// already exists in MatchBox are listed. (`asc`, `chr`, `serializeJSON` and
/// `deserializeJSON` exist in MatchBox under their CF names, so they stay.)
/// Keys are lowercase; matching ignores case, as CF does.
const BIF_RENAMES: &[(&str, &str)] = &[
    ("gettemplatepath", "getBaseTemplatePath"),
    ("valuearray", "queryColumnData"),
    ("querygetrow", "queryRowData"),
];

/// CF-only variable names and their BoxLang equivalents (all of BoxLang's
/// `identifierMap`).
const IDENTIFIER_RENAMES: &[(&str, &str)] = &[
    ("cfcatch", "bxcatch"),
    ("cfthread", "bxthread"),
    ("cffile", "bxfile"),
    ("cfftp", "bxftp"),
    ("cfhttp", "bxhttp"),
    ("cfquery", "bxquery"),
    ("cfdocument", "bxdocument"),
    ("cfstoredproc", "bxstoredproc"),
    ("thistag", "thiscomponent"),
];

/// Rename CF-only names in `stmts` to their BoxLang equivalents.
pub fn transpile(stmts: &mut [Statement]) {
    let mut pass = Transpiler {
        user_functions: collect_function_names(stmts),
        scopes: Vec::new(),
    };
    pass.stmts(stmts);
}

/// Look `name` up in a rename table, ignoring case.
fn lookup(table: &[(&str, &'static str)], name: &str) -> Option<&'static str> {
    table
        .iter()
        .find(|(from, _)| name.eq_ignore_ascii_case(from))
        .map(|(_, to)| *to)
}

/// Names (lowercase) of functions the file declares itself, at the top level
/// or as class members. A call to one of those is never renamed, so a user
/// function called `valueArray` keeps working.
fn collect_function_names(stmts: &[Statement]) -> HashSet<String> {
    let mut names = HashSet::new();
    for stmt in stmts {
        match &stmt.kind {
            StatementKind::FunctionDecl { name, .. } => {
                names.insert(name.to_ascii_lowercase());
            }
            StatementKind::ClassDecl { members, .. } => {
                for member in members {
                    if let ClassMember::Statement(s) = member
                        && let StatementKind::FunctionDecl { name, .. } = &s.kind
                    {
                        names.insert(name.to_ascii_lowercase());
                    }
                }
            }
            _ => {}
        }
    }
    names
}

struct Transpiler {
    user_functions: HashSet<String>,
    /// Parameter names (lowercase) of the functions we are currently inside,
    /// innermost last. A name declared as a parameter is the user's own
    /// variable there, so it is not renamed (BoxLang's
    /// `isInFunctionWithArgNamed`).
    scopes: Vec<HashSet<String>>,
}

impl Transpiler {
    fn stmts(&mut self, stmts: &mut [Statement]) {
        for stmt in stmts {
            self.stmt(stmt);
        }
    }

    fn rename_identifier(&mut self, name: &mut String) {
        let lower = name.to_ascii_lowercase();
        if self.scopes.iter().any(|scope| scope.contains(&lower)) {
            return;
        }
        if let Some(to) = lookup(IDENTIFIER_RENAMES, name) {
            *name = to.to_string();
        }
    }

    // Every variant is listed on purpose, with no `_ =>` catch-all: if someone
    // adds a new kind of statement to the AST, this stops compiling until they
    // decide how the rename pass should handle it.
    fn stmt(&mut self, stmt: &mut Statement) {
        match &mut stmt.kind {
            StatementKind::Import { .. }
            | StatementKind::Continue(_)
            | StatementKind::Break(_)
            | StatementKind::Rethrow => {}
            StatementKind::ClassDecl { members, .. } => {
                for member in members {
                    match member {
                        ClassMember::Statement(s) => self.stmt(s),
                        ClassMember::Property(_) => {}
                    }
                }
            }
            StatementKind::InterfaceDecl { members, .. } => self.stmts(members),
            StatementKind::FunctionDecl {
                attributes,
                params,
                body,
                ..
            } => {
                for attribute in attributes {
                    self.args(&mut attribute.args);
                }
                self.function(params, body);
            }
            StatementKind::ForLoop {
                item,
                index,
                collection,
                body,
            } => {
                // `for (cfquery in rows)` declares the loop variable by name,
                // so it gets the same rename as its uses in the body.
                self.rename_identifier(item);
                if let Some(index) = index {
                    self.rename_identifier(index);
                }
                self.expr(collection);
                self.stmts(body);
            }
            StatementKind::ForClassic {
                init,
                condition,
                update,
                body,
            } => {
                if let Some(init) = init {
                    self.stmt(init);
                }
                if let Some(condition) = condition {
                    self.expr(condition);
                }
                if let Some(update) = update {
                    self.expr(update);
                }
                self.stmts(body);
            }
            StatementKind::WhileLoop { condition, body }
            | StatementKind::DoWhile { condition, body } => {
                self.expr(condition);
                self.stmts(body);
            }
            // The label names a loop, not a variable, so only the body is walked.
            StatementKind::Labeled { body, .. } => self.stmts(body),
            StatementKind::Component {
                attributes, body, ..
            } => {
                for (key, value) in attributes {
                    // An attribute name like `name=` is not a variable, same as
                    // a bare-word struct key; computed names are still walked.
                    if !matches!(key.kind, ExpressionKind::Identifier(_)) {
                        self.expr(key);
                    }
                    self.expr(value);
                }
                if let Some(body) = body {
                    self.stmts(body);
                }
            }
            StatementKind::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.expr(condition);
                self.stmts(then_branch);
                if let Some(branch) = else_branch {
                    self.stmts(branch);
                }
            }
            StatementKind::Switch {
                value,
                cases,
                default_case,
            } => {
                self.expr(value);
                for case in cases {
                    self.expr(&mut case.value);
                    self.stmts(&mut case.body);
                }
                if let Some(body) = default_case {
                    self.stmts(body);
                }
            }
            StatementKind::Return(value) | StatementKind::Throw(value) => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            StatementKind::Assert { condition, message } => {
                self.expr(condition);
                if let Some(message) = message {
                    self.expr(message);
                }
            }
            StatementKind::Param { default, .. } => {
                if let Some(default) = default {
                    self.expr(default);
                }
            }
            StatementKind::Not(e)
            | StatementKind::Include(e)
            | StatementKind::BufferOutput(e)
            | StatementKind::Expression(e) => self.expr(e),
            StatementKind::Destructure { source, .. } => self.expr(source),
            StatementKind::TryCatch {
                try_branch,
                catches,
                finally_branch,
            } => {
                self.stmts(try_branch);
                for catch in catches {
                    // `catch (any cfcatch)` declares the variable by name.
                    self.rename_identifier(&mut catch.exception_var);
                    self.stmts(&mut catch.body);
                }
                if let Some(branch) = finally_branch {
                    self.stmts(branch);
                }
            }
            StatementKind::VariableDecl { name, value } => {
                self.rename_identifier(name);
                self.expr(value);
            }
        }
    }

    fn expr(&mut self, expr: &mut Expression) {
        match &mut expr.kind {
            ExpressionKind::Identifier(name) => self.rename_identifier(name),
            ExpressionKind::FunctionCall { base, args } => {
                if let ExpressionKind::Identifier(name) = &mut base.kind {
                    // A plain call like `valueArray(q, "id")`: rename the BIF,
                    // unless the file declares its own function with that name.
                    if !self.user_functions.contains(&name.to_ascii_lowercase())
                        && let Some(to) = lookup(BIF_RENAMES, name)
                    {
                        *name = to.to_string();
                    }
                } else {
                    // e.g. `obj.method(...)`: the part before the call may
                    // contain variables, such as `cfhttp.getResult()`.
                    self.expr(base);
                }
                self.args(args);
            }
            ExpressionKind::New { args, .. } => self.args(args),
            ExpressionKind::Assignment { target, value } => {
                self.target(target);
                self.expr(value);
            }
            ExpressionKind::Binary { left, right, .. } | ExpressionKind::Elvis { left, right } => {
                self.expr(left);
                self.expr(right);
            }
            ExpressionKind::UnaryNot(e)
            | ExpressionKind::UnaryBitwiseNot(e)
            | ExpressionKind::Spread(e) => self.expr(e),
            ExpressionKind::Ternary {
                condition,
                then_expr,
                else_expr,
            } => {
                self.expr(condition);
                self.expr(then_expr);
                self.expr(else_expr);
            }
            ExpressionKind::ArrayAccess { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            // Only the base is a variable. In `cfcatch.message`, `cfcatch` is
            // renamed; `message` is a key and stays as written.
            ExpressionKind::MemberAccess { base, .. }
            | ExpressionKind::SafeMemberAccess { base, .. } => self.expr(base),
            ExpressionKind::Prefix { target, .. } => self.target(target),
            ExpressionKind::Postfix { base, .. } => self.expr(base),
            ExpressionKind::Literal(literal) => self.literal(literal),
        }
    }

    fn literal(&mut self, literal: &mut Literal) {
        match literal {
            Literal::Number(_) | Literal::Boolean(_) | Literal::Null => {}
            Literal::String(parts) => {
                // Interpolation: "#cfcatch.message#"
                for part in parts {
                    if let StringPart::Expression(e) = part {
                        self.expr(e);
                    }
                }
            }
            Literal::Array(items) => {
                for item in items {
                    self.expr(item);
                }
            }
            Literal::Struct(entries) => {
                for (key, value) in entries {
                    // A bare-word key like `{ cfcatch : 1 }` is a key name,
                    // not a variable, so it is left alone. Computed keys are
                    // still walked.
                    if !matches!(key.kind, ExpressionKind::Identifier(_)) {
                        self.expr(key);
                    }
                    self.expr(value);
                }
            }
            Literal::Function { params, body, .. } => self.function(params, body),
        }
    }

    fn target(&mut self, target: &mut AssignmentTarget) {
        match target {
            AssignmentTarget::Identifier(name) => self.rename_identifier(name),
            AssignmentTarget::Member { base, .. } => self.expr(base),
            AssignmentTarget::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
        }
    }

    fn body(&mut self, body: &mut FunctionBody) {
        match body {
            FunctionBody::Block(stmts) | FunctionBody::Component(stmts) => self.stmts(stmts),
            FunctionBody::Expression(e) => self.expr(e),
            FunctionBody::Abstract => {}
        }
    }

    /// A function declaration or closure: its parameter names shadow the
    /// rename tables inside its body.
    fn function(&mut self, params: &mut [FunctionParam], body: &mut FunctionBody) {
        // Defaults are inside the function too: in `f(cfhttp, b = cfhttp)`
        // the default refers to the parameter.
        self.scopes
            .push(params.iter().map(|p| p.name.to_ascii_lowercase()).collect());
        self.params(params);
        self.body(body);
        self.scopes.pop();
    }

    fn params(&mut self, params: &mut [FunctionParam]) {
        for param in params {
            if let Some(default) = &mut param.default_value {
                self.expr(default);
            }
        }
    }

    fn args(&mut self, args: &mut [Argument]) {
        // Named-argument names (`f(cfhttp = 1)`) are parameter names, not
        // variables, so only the values are walked.
        for arg in args {
            self.expr(&mut arg.value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;

    /// An AST's debug text with every source position reduced to `Span`.
    /// `valueArray` and `queryColumnData` build the same tree, but have
    /// different lengths, so their byte spans differ.
    fn shape<T: std::fmt::Debug>(ast: &T) -> String {
        let text = format!("{ast:?}");
        let mut out = String::with_capacity(text.len());
        let mut rest = text.as_str();
        while let Some(start) = rest.find("Span {") {
            out.push_str(&rest[..start]);
            out.push_str("Span");
            let end = rest[start..].find('}').expect("Span debug text is closed");
            rest = &rest[start + end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Parse `cf`, run the rename pass, and check it equals `bx` parsed as-is.
    fn assert_renames_to(cf: &str, bx: &str) {
        let mut cf_ast = parse(cf, None).unwrap();
        transpile(&mut cf_ast);
        let bx_ast = parse(bx, None).unwrap();
        assert_eq!(shape(&cf_ast), shape(&bx_ast), "CF source: {cf}");
    }

    #[test]
    fn renames_cf_only_bifs() {
        assert_renames_to(
            "x = valueArray(q, \"id\");",
            "x = queryColumnData(q, \"id\");",
        );
        assert_renames_to("x = queryGetRow(q, 1);", "x = queryRowData(q, 1);");
        assert_renames_to("x = getTemplatePath();", "x = getBaseTemplatePath();");
        // CF is case-insensitive.
        assert_renames_to(
            "x = VALUEARRAY(q, \"id\");",
            "x = queryColumnData(q, \"id\");",
        );
    }

    #[test]
    fn renames_cf_identifiers() {
        for (from, to) in IDENTIFIER_RENAMES {
            assert_renames_to(&format!("x = {from}.result;"), &format!("x = {to}.result;"));
        }
        assert_renames_to("x = CFCATCH.message;", "x = bxcatch.message;");
    }

    #[test]
    fn renames_reach_every_kind_of_statement() {
        // One line each side, so the line numbers in both trees match.
        assert_renames_to(
            "function f(a = cffile) { if (cfhttp) { while (cfquery) { x = valueArray(q, \"c\"); } } return [cfthread, { k : thistag }, \"#cfcatch.message#\", cfquery ? cffile : cfhttp]; }",
            "function f(a = bxfile) { if (bxhttp) { while (bxquery) { x = queryColumnData(q, \"c\"); } } return [bxthread, { k : thiscomponent }, \"#bxcatch.message#\", bxquery ? bxfile : bxhttp]; }",
        );
        assert_renames_to(
            "try { x = 1; } catch (any cfcatch) { y = cfcatch.message; } finally { z = cffile; }",
            "try { x = 1; } catch (any bxcatch) { y = bxcatch.message; } finally { z = bxfile; }",
        );
        assert_renames_to(
            "switch (cfhttp.status) { case 1: x = cfquery; break; default: y = () => cffile; }",
            "switch (bxhttp.status) { case 1: x = bxquery; break; default: y = () => bxfile; }",
        );
        assert_renames_to(
            "class { function g() { for (row in cfquery) { cfhttp.count++; } } }",
            "class { function g() { for (row in bxquery) { bxhttp.count++; } } }",
        );
    }

    #[test]
    fn user_function_named_like_a_bif_is_not_renamed() {
        // The file defines its own valueArray, so calls to it stay as written.
        let src = "function valueArray(x) { return x; } y = valueArray(1);";
        assert_renames_to(src, src);
    }

    #[test]
    fn keys_and_member_names_are_not_renamed() {
        // `cfcatch` here is a struct key and a property name, not a variable.
        let src = "s = { cfcatch : 1 }; y = obj.cfhttp; f(cfquery = 2);";
        assert_renames_to(src, src);
    }

    #[test]
    fn parameter_names_are_not_renamed_inside_their_function() {
        // The parameter is the user's own variable inside the function.
        for src in [
            "function show(cfhttp) { return \"got \" & cfhttp; }",
            "x = (cffile) => cffile * 2;",
            "x = function(cfquery) { return cfquery; };",
            "function f(cfhttp, b = cfhttp) { return b; }",
        ] {
            assert_renames_to(src, src);
        }
        // Outside the function, the same name is still renamed.
        assert_renames_to(
            "function f(cfhttp) { return cfhttp; } y = cfhttp;",
            "function f(cfhttp) { return cfhttp; } y = bxhttp;",
        );
    }

    #[test]
    fn loop_variables_are_renamed_with_their_uses() {
        assert_renames_to(
            "for (cfquery in [1, 2]) { y = cfquery; }",
            "for (bxquery in [1, 2]) { y = bxquery; }",
        );
    }

    #[test]
    fn parse_source_runs_the_rename_pass_for_cf_files() {
        use crate::cf::{SourceKind, parse_source};
        let cf = parse_source(
            SourceKind::CfScript,
            "component { function f() { return valueArray(q, \"id\"); } }",
            None,
        )
        .unwrap();
        let bx = parse(
            "class { function f() { return queryColumnData(q, \"id\"); } }",
            None,
        )
        .unwrap();
        assert_eq!(shape(&cf), shape(&bx));
    }
}
