use crate::ast::*;
use crate::tokenizer::*;
use anyhow::Result;

pub fn parse_template(source: &str, filename: Option<&str>) -> Result<Vec<Statement>> {
    let lexed = lex_template(source);
    match TemplateParser::new(source, lexed.tokens(), filename).parse() {
        Ok(stmts) => Ok(stmts),
        Err(err) => {
            // Attach source text so snippet rendering can show the offending
            // line, mirroring the script parser's behavior.
            if let Some(parse_err) = err.downcast_ref::<crate::parser::ParseError>() {
                if parse_err.source.is_none() {
                    let mut enriched = parse_err.clone();
                    enriched.source = Some(source.to_string());
                    return Err(enriched.into());
                }
            }
            Err(err)
        }
    }
}

struct TemplateParser<'a> {
    source: &'a str,
    tokens: &'a [SyntaxToken],
    pos: usize,
    pending: Vec<Statement>,
    filename: Option<&'a str>,
}

impl<'a> TemplateParser<'a> {
    fn new(source: &'a str, tokens: &'a [SyntaxToken], filename: Option<&'a str>) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
            pending: Vec::new(),
            filename,
        }
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.tokens.get(self.pos).map(|t| t.kind)
    }

    fn peek_lexeme(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|t| &self.source[t.span.start..t.span.end])
    }

    /// Line number of the token under the cursor, falling back to the last
    /// token's line at end of input. Used so template AST nodes carry real
    /// line numbers instead of the hardcoded `0`.
    fn peek_line(&self) -> u32 {
        self.tokens
            .get(self.pos)
            .map(|t| t.span.line)
            .or_else(|| self.tokens.last().map(|t| t.span.line))
            .unwrap_or(1)
    }

    fn advance_lexeme(&mut self) -> Option<String> {
        let lexeme = self
            .tokens
            .get(self.pos)
            .map(|t| self.source[t.span.start..t.span.end].to_string());
        self.pos += 1;
        lexeme
    }

    fn parse(&mut self) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        while self.pos < self.tokens.len() || !self.pending.is_empty() {
            if let Some(stmt) = self.pending.pop() {
                stmts.push(stmt);
                continue;
            }
            if let Some(stmt) = self.parse_template_statement()? {
                stmts.push(stmt);
            }
        }
        Ok(stmts)
    }

    fn parse_template_statement(&mut self) -> Result<Option<Statement>> {
        match self.peek_kind() {
            Some(TokenKind::ContentText) => {
                // Use the line of the first text token for the whole run.
                let line = self.peek_line();
                let mut parts = Vec::new();
                while self.peek_kind() == Some(TokenKind::ContentText) {
                    if let Some(lexeme) = self.advance_lexeme() {
                        let text = decode_template_text(&lexeme);
                        if !text.is_empty() {
                            parts.push(StringPart::Text(text));
                        }
                    }
                }
                if parts.is_empty() {
                    return Ok(None);
                }
                let expr = Expression::new(ExpressionKind::Literal(Literal::String(parts)), line);
                Ok(Some(Statement::new(
                    StatementKind::BufferOutput(expr),
                    line,
                )))
            }
            Some(TokenKind::ScriptStart) => {
                self.parse_script_island()?;
                Ok(None)
            }
            Some(TokenKind::ComponentName) => {
                if self.is_closing_tag() { anyhow::bail!("Unexpected closing component tag: {}", self.peek_lexeme().unwrap_or("")); }
                let name = self.advance_lexeme().unwrap_or_default();
                self.parse_component(&name)
            }
            Some(TokenKind::InterpStart) => {
                let line = self.peek_line();
                self.pos += 1;
                let expression = self.parse_interpolation_expression()?;
                Ok(Some(Statement::new(StatementKind::BufferOutput(expression), line)))
            }
            Some(TokenKind::Less) => {
                let line = self.peek_line();
                self.pos += 1;
                let expr = Expression::new(
                    ExpressionKind::Literal(Literal::String(vec![StringPart::Text(
                        "<".to_string(),
                    )])),
                    line,
                );
                Ok(Some(Statement::new(StatementKind::BufferOutput(expr), line)))
            }
            Some(TokenKind::ComponentClose)
            | Some(TokenKind::ComponentSelfClose)
            | Some(TokenKind::InterpEnd)
            | Some(TokenKind::ScriptEnd) => {
                self.pos += 1;
                Ok(None)
            }
            None => Ok(None),
            _ => {
                // Non-template tokens: script island content or #expr# interpolation
                // Skip for now — needs template/script token boundary markers
                self.pos += 1;
                Ok(None)
            }
        }
    }

    fn parse_component(&mut self, name: &str) -> Result<Option<Statement>> {
        match name.to_ascii_lowercase().as_str() {
            "set" => self.parse_set_tag(),
            "if" => self.parse_if_tag(),
            "loop" => self.parse_loop_tag(),
            "return" => self.parse_return_tag(),
            "break" => self.parse_break_continue(true),
            "continue" => self.parse_break_continue(false),
            "try" => self.parse_try_tag(),
            "switch" => self.parse_switch_tag(),
            "include" => self.parse_include_tag(),
            "import" => self.parse_import_tag(),
            "throw" => self.parse_throw_tag(),
            "rethrow" => self.parse_simple_tag(StatementKind::Rethrow),
            "function" => self.parse_function_tag(),
            "output" => self.parse_output_tag(),
            "script" => {
                self.skip_to_close();
                Ok(None)
            }
            _ => self.parse_module_component(name),
        }
    }

    /// The expression after `=` in a tag attribute (`a="#x#"` is the expression `x`).
    fn parse_attribute_value(&mut self) -> Result<Expression> {
        let text = self.advance_lexeme().ok_or_else(|| anyhow::anyhow!("Missing component attribute value"))?;
        let ast = crate::parser::parse(&format!("{text};"), self.filename)?;
        let Some(Statement {kind: StatementKind::Expression(mut expression), ..}) = ast.into_iter().next() else { anyhow::bail!("Invalid component attribute value"); };
        if let ExpressionKind::Literal(Literal::String(parts)) = &expression.kind {
            if let [StringPart::Expression(value)] = parts.as_slice() { expression = value.clone(); }
        }
        Ok(expression)
    }

    /// `key=value` attributes up to the end of the opening tag, which is consumed.
    /// Returns the attributes (keys lowercased) and whether the tag was self-closing.
    fn parse_tag_attributes(&mut self, line: u32) -> Result<(Vec<(String, Expression)>, bool)> {
        let mut attributes = Vec::new();
        while self.peek_kind() == Some(TokenKind::Identifier) {
            let key = self.advance_lexeme().unwrap().to_ascii_lowercase();
            let value = if self.peek_kind() == Some(TokenKind::Equal) {
                self.pos += 1;
                self.parse_attribute_value()?
            } else {
                Expression::new(ExpressionKind::Literal(Literal::Boolean(true)), line)
            };
            attributes.push((key, value));
        }
        let self_closing = match self.peek_kind() {
            Some(TokenKind::ComponentSelfClose) => true,
            Some(TokenKind::ComponentClose) => false,
            _ => anyhow::bail!("Invalid component tag"),
        };
        self.pos += 1;
        Ok((attributes, self_closing))
    }

    fn parse_module_component(&mut self, name: &str) -> Result<Option<Statement>> {
        let line = self.peek_line();
        let mut attributes = Vec::new();
        while self.peek_kind() == Some(TokenKind::Identifier) {
            let key = self.advance_lexeme().unwrap();
            let value = if self.peek_kind() == Some(TokenKind::Equal) {
                self.pos += 1;
                let text = self.advance_lexeme().ok_or_else(|| anyhow::anyhow!("Missing component attribute value"))?;
                let ast = crate::parser::parse(&format!("{text};"), self.filename)?;
                let Some(Statement {kind: StatementKind::Expression(mut expression), ..}) = ast.into_iter().next() else { anyhow::bail!("Invalid component attribute value"); };
                if let ExpressionKind::Literal(Literal::String(parts)) = &expression.kind {
                    if let [StringPart::Expression(value)] = parts.as_slice() { expression = value.clone(); }
                }
                expression
            } else { Expression::new(ExpressionKind::Literal(Literal::Boolean(true)), line) };
            attributes.push((Expression::new(ExpressionKind::Literal(Literal::String(vec![StringPart::Text(key)])), line), value));
        }
        let body = match self.peek_kind() {
            Some(TokenKind::ComponentSelfClose) => { self.pos += 1; None }
            Some(TokenKind::ComponentClose) => {
                self.pos += 1;
                let lower = name.to_ascii_lowercase();
                let has_closing = lower == "content" && self.tokens[self.pos..].iter().any(|token| {
                    token.kind == TokenKind::ComponentName
                        && self.source[token.span.start..token.span.end].eq_ignore_ascii_case(name)
                        && self.source[..token.span.start].rsplit_once('<').is_some_and(|(_, prefix)| prefix.starts_with('/'))
                });
                if matches!(lower.as_str(), "header" | "abort" | "location" | "cookie") || (lower == "content" && !has_closing) {
                    return Ok(Some(component_statement(name, attributes, None, line)?));
                }
                let body = self.parse_template_body(&[name])?;
                if self.peek_kind() != Some(TokenKind::ComponentName) || !self.is_closing_tag() { anyhow::bail!("Missing closing component tag: {name}"); }
                self.skip_closing(name);
                Some(body)
            }
            _ => anyhow::bail!("Invalid component tag: {name}"),
        };
        Ok(Some(component_statement(name, attributes, body, line)?))
    }

    fn parse_output_tag(&mut self) -> Result<Option<Statement>> {
        self.skip_to_close();
        let mut body = self.parse_template_body(&["output"])?;
        self.skip_closing("output");
        body.reverse();
        let first = body.pop();
        self.pending.extend(body);
        Ok(first)
    }

    fn parse_interpolation_expression(&mut self) -> Result<Expression> {
        let line = self.peek_line();
        let expr_text = self.collect_text_until(TokenKind::InterpEnd);
        self.skip_expected(TokenKind::InterpEnd);

        if expr_text.trim().is_empty() {
            return Ok(Expression::new(
                ExpressionKind::Literal(Literal::Null),
                line,
            ));
        }

        let stmts = crate::parser::parse(&expr_text, None)?;
        let expr = stmts
            .into_iter()
            .next()
            .and_then(|stmt| {
                if let StatementKind::Expression(expr) = stmt.kind {
                    Some(expr)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                Expression::new(ExpressionKind::Literal(Literal::Null), line)
            });

        Ok(expr)
    }

    fn parse_script_island(&mut self) -> Result<()> {
        self.skip_expected(TokenKind::ScriptStart);
        // Line of the first content token, used as the offset so errors inside
        // the island line up with the template. The opener (`<bx:script>`)
        // and the content may be on the same or different lines; the token
        // under the cursor after skipping the opener tells us the truth.
        let content_start_line = self.peek_line();
        let script_text = self.collect_text_until(TokenKind::ScriptEnd);
        self.skip_expected(TokenKind::ScriptEnd);

        if script_text.trim().is_empty() {
            return Ok(());
        }

        // Re-parse the island content treating its first line as
        // `content_start_line` in the template. parse_with_line_offset handles
        // the translation; the returned ParseError already has correct lines.
        let mut stmts = crate::parser::parse_with_line_offset(
            &script_text,
            self.filename,
            content_start_line.saturating_sub(1),
        )?;
        stmts.reverse();
        self.pending.extend(stmts);
        Ok(())
    }

    fn parse_set_tag(&mut self) -> Result<Option<Statement>> {
        let mut expr_text = String::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::ComponentClose) | Some(TokenKind::ComponentSelfClose) => {
                    self.pos += 1;
                    break;
                }
                Some(TokenKind::Identifier) | Some(TokenKind::Number)
                | Some(TokenKind::String) | Some(TokenKind::Equal)
                | Some(TokenKind::Plus) | Some(TokenKind::Minus) | Some(TokenKind::Star)
                | Some(TokenKind::Slash) | Some(TokenKind::Dot) | Some(TokenKind::Ampersand)
                | Some(TokenKind::Less) | Some(TokenKind::Greater)
                | Some(TokenKind::LeftParen) | Some(TokenKind::RightParen) => {
                    let lex = self.advance_lexeme().unwrap_or_default();
                    if !expr_text.is_empty() { expr_text.push(' '); }
                    expr_text.push_str(&lex);
                }
                _ => { self.pos += 1; }
            }
        }
        if expr_text.trim().is_empty() { return Ok(None); }
        if let Ok(stmts) = crate::parser::parse(&expr_text, None) {
            if let Some(first) = stmts.into_iter().next() {
                return Ok(Some(first));
            }
        }
        Ok(None)
    }

    fn parse_if_tag(&mut self) -> Result<Option<Statement>> {
        let line = self.peek_line();
        // Collect condition expression
        let mut cond_text = String::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::ComponentClose) => { self.pos += 1; break; }
                Some(TokenKind::ComponentSelfClose) => { self.pos += 1; return Ok(None); }
                Some(TokenKind::Identifier) | Some(TokenKind::Number) | Some(TokenKind::String)
                | Some(TokenKind::Equal) | Some(TokenKind::Plus) | Some(TokenKind::Minus)
                | Some(TokenKind::Star) | Some(TokenKind::Slash) | Some(TokenKind::Dot)
                | Some(TokenKind::Less) | Some(TokenKind::Greater) | Some(TokenKind::Ampersand)
                | Some(TokenKind::EqualEqual) | Some(TokenKind::BangEqual)
                | Some(TokenKind::LessEqual) | Some(TokenKind::GreaterEqual)
                | Some(TokenKind::AmpAmp) | Some(TokenKind::PipePipe)
                | Some(TokenKind::LeftParen) | Some(TokenKind::RightParen)
                | Some(TokenKind::Bang) => {
                    let lex = self.advance_lexeme().unwrap_or_default();
                    if !cond_text.is_empty() { cond_text.push(' '); }
                    cond_text.push_str(&lex);
                }
                _ => { self.pos += 1; }
            }
        }

        // Parse condition
        let condition = if !cond_text.trim().is_empty() {
            if let Ok(stmts) = crate::parser::parse(&cond_text, None) {
                stmts.into_iter().next().and_then(|s| {
                    if let StatementKind::Expression(expr) = s.kind { Some(expr) } else { None }
                })
            } else { None }
        } else { None };

        // Parse body until </bx:if>, <bx:elseif>, or <bx:else>
        let then_branch = self.parse_template_body(&["elseif", "else", "if"])?;
        let mut else_branch = None;

        // Check for elseif or else
        if self.peek_kind() == Some(TokenKind::ComponentName) {
            let next_name = self.peek_lexeme().unwrap_or("");
            if next_name == "elseif" {
                self.pos += 1;
                let elseif_stmt = self.parse_if_tag()?;
                else_branch = elseif_stmt.map(|s| vec![s]);
            } else if next_name == "else" {
                self.pos += 1;
                self.skip_to_close();
                let else_body = self.parse_template_body(&["if"])?;
                else_branch = Some(else_body);
            }
        }

        // Skip closing </bx:if> if present
        if self.peek_kind() == Some(TokenKind::ComponentName) {
            let name = self.peek_lexeme().unwrap_or("");
            if name == "if" {
                self.pos += 1;
                self.skip_to_close();
            }
        }

        let condition = condition.unwrap_or_else(|| Expression::new(
            ExpressionKind::Literal(Literal::Boolean(true)), line,
        ));

        Ok(Some(Statement::new(
            StatementKind::If { condition, then_branch, else_branch },
            line,
        )))
    }

    fn parse_template_body(&mut self, end_tags: &[&str]) -> Result<Vec<Statement>> {
        let mut stmts = Vec::new();
        loop {
            if let Some(statement) = self.pending.pop() { stmts.push(statement); continue; }
            if self.peek_kind() == Some(TokenKind::ComponentName) {
                let name = self.peek_lexeme().unwrap_or("");
                if end_tags.iter().any(|end| end.eq_ignore_ascii_case(name)) && (self.is_closing_tag() || matches!(name, "elseif" | "else" | "catch" | "finally" | "case" | "defaultcase")) { break; }
                if self.is_closing_tag() { anyhow::bail!("Unexpected closing component tag: {name}"); }
            }
            if self.peek_kind().is_none() { break; }
            if let Some(stmt) = self.parse_template_statement()? {
                stmts.push(stmt);
            }
        }
        Ok(stmts)
    }

    fn parse_loop_tag(&mut self) -> Result<Option<Statement>> {
        // <bx:loop from="1" to="3" index="i"> / array="#a#" item="x" / list="a,b" item="x" /
        // collection="#struct#" item="key" / condition="expr"
        let line = self.peek_line();
        let (attributes, self_closing) = self.parse_tag_attributes(line)?;
        let body = if self_closing {
            Vec::new()
        } else {
            let body = self.parse_template_body(&["loop"])?;
            self.skip_closing("loop");
            body
        };
        let attribute = |name: &str| attributes.iter().find(|(key, _)| key == name).map(|(_, value)| value.clone());
        let variable = |name: &str| attribute(name).and_then(|value| literal_text(&value));
        let ident = |name: &str| Expression::new(ExpressionKind::Identifier(name.to_string()), line);

        if let Some(condition) = attribute("condition") {
            let condition = match literal_text(&condition) {
                Some(text) => script_expression(&text, self.filename)?,
                None => condition,
            };
            return Ok(Some(Statement::new(StatementKind::WhileLoop { condition, body }, line)));
        }

        if let (Some(from), Some(to)) = (attribute("from"), attribute("to")) {
            let index = variable("index").ok_or_else(|| anyhow::anyhow!("<bx:loop from/to> requires an index attribute"))?;
            // Unspellable local names keep nested loop state out of the template's variables.
            let cursor = format!("@loop:{}:cursor", self.pos);
            let limit = format!("@loop:{}:limit", self.pos);
            let increment = format!("@loop:{}:step", self.pos);
            let number = |value| Expression::new(ExpressionKind::Literal(Literal::Number(value)), line);
            let binary = |left, operator: &str, right| Expression::new(
                ExpressionKind::Binary { left: Box::new(left), operator: operator.into(), right: Box::new(right) }, line,
            );
            let condition = binary(
                binary(binary(ident(&increment), ">", number(0.0)), "&&", binary(ident(&cursor), "<=", ident(&limit))),
                "||",
                binary(binary(ident(&increment), "<", number(0.0)), "&&", binary(ident(&cursor), ">=", ident(&limit))),
            );
            let update = Expression::new(ExpressionKind::Assignment {
                target: AssignmentTarget::Identifier(cursor.clone()),
                value: Box::new(binary(ident(&cursor), "+", ident(&increment))),
            }, line);
            let mut loop_body = vec![Statement::new(StatementKind::Expression(Expression::new(
                ExpressionKind::Assignment { target: AssignmentTarget::Identifier(index), value: Box::new(ident(&cursor)) }, line,
            )), line)];
            loop_body.extend(body);
            let mut statements = vec![
                Statement::new(StatementKind::VariableDecl { name: cursor, value: number_literal(from) }, line),
                Statement::new(StatementKind::VariableDecl { name: limit, value: number_literal(to) }, line),
                Statement::new(StatementKind::VariableDecl {
                    name: increment,
                    value: attribute("step").map(number_literal).unwrap_or_else(|| number(1.0)),
                }, line),
            ];
            statements.push(Statement::new(StatementKind::ForClassic {
                init: None, condition: Some(condition), update: Some(update), body: loop_body,
            }, line));
            return Ok(Some(Statement::new(StatementKind::If {
                condition: Expression::new(ExpressionKind::Literal(Literal::Boolean(true)), line),
                then_branch: statements, else_branch: None,
            }, line)));
        }

        let collection = if let Some(array) = attribute("array").or_else(|| attribute("collection")) {
            Some(array)
        } else if let Some(list) = attribute("list") {
            let delimiters = attribute("delimiters").unwrap_or_else(|| {
                Expression::new(ExpressionKind::Literal(Literal::String(vec![StringPart::Text(",".to_string())])), line)
            });
            Some(Expression::new(
                ExpressionKind::FunctionCall {
                    base: Box::new(ident("listToArray")),
                    args: vec![
                        Argument { name: None, value: list },
                        Argument { name: None, value: delimiters },
                    ],
                },
                line,
            ))
        } else {
            None
        };
        if let Some(collection) = collection {
            let item = variable("item").or_else(|| variable("index")).ok_or_else(|| anyhow::anyhow!("<bx:loop> requires an item attribute"))?;
            let index = if variable("item").is_some() { variable("index") } else { None };
            return Ok(Some(Statement::new(StatementKind::ForLoop { item, index, collection, body }, line)));
        }

        anyhow::bail!("Unsupported <bx:loop> attributes: use from/to, array, list, collection or condition")
    }

    fn parse_return_tag(&mut self) -> Result<Option<Statement>> {
        let mut expr_text = String::new();
        loop {
            match self.peek_kind() {
                Some(TokenKind::ComponentClose) | Some(TokenKind::ComponentSelfClose) => {
                    self.pos += 1; break;
                }
                _ => {
                    if !expr_text.is_empty() { expr_text.push(' '); }
                    expr_text.push_str(&self.advance_lexeme().unwrap_or_default());
                }
            }
        }
        let expr = if expr_text.trim().is_empty() {
            None
        } else if let Ok(stmts) = crate::parser::parse(&expr_text, None) {
            stmts.into_iter().next().and_then(|s| if let StatementKind::Expression(e) = s.kind { Some(e) } else { None })
        } else { None };
        Ok(Some(Statement::new(StatementKind::Return(expr), self.peek_line())))
    }

    fn parse_break_continue(&mut self, is_break: bool) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        Ok(Some(Statement::new(
            if is_break { StatementKind::Break(None) } else { StatementKind::Continue(None) }, line,
        )))
    }

    fn parse_try_tag(&mut self) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        let try_branch = self.parse_template_body(&["catch", "finally", "try"])?;
        let mut catches = Vec::new();
        let mut finally_branch = None;
        // Handle catch blocks
        loop {
            if self.peek_kind() == Some(TokenKind::ComponentName) {
                let name = self.peek_lexeme().unwrap_or("");
                if name == "catch" {
                    self.pos += 1;
                    self.skip_to_close();
                    let body = self.parse_template_body(&["catch", "finally", "try"])?;
                    catches.push(CatchBlock { exception_var: "e".to_string(), body });
                    continue;
                }
                if name == "finally" {
                    self.pos += 1;
                    self.skip_to_close();
                    finally_branch = Some(self.parse_template_body(&["try"])?);
                    break;
                }
            }
            break;
        }
        self.skip_closing("try");
        Ok(Some(Statement::new(StatementKind::TryCatch { try_branch, catches, finally_branch }, line)))
    }

    fn parse_switch_tag(&mut self) -> Result<Option<Statement>> {
        // <bx:switch expression="#expr#"> <bx:case value="1">...</bx:case> <bx:defaultcase>...</bx:defaultcase> </bx:switch>
        let line = self.peek_line();
        let (attributes, self_closing) = self.parse_tag_attributes(line)?;
        let value = attributes
            .iter()
            .find(|(key, _)| key == "expression")
            .map(|(_, value)| value.clone())
            .ok_or_else(|| anyhow::anyhow!("<bx:switch> requires an expression attribute"))?;
        let mut cases = Vec::new();
        let mut default_case = None;
        while !self_closing {
            match self.peek_kind() {
                Some(TokenKind::ContentText) => {
                    // Whitespace between the case tags is not output.
                    let text = self.peek_lexeme().unwrap_or("");
                    if !text.trim().is_empty() {
                        anyhow::bail!("Only <bx:case> and <bx:defaultcase> are allowed inside <bx:switch>");
                    }
                    self.pos += 1;
                }
                Some(TokenKind::ComponentName) if !self.is_closing_tag() => {
                    let name = self.advance_lexeme().unwrap_or_default().to_ascii_lowercase();
                    match name.as_str() {
                        "case" => {
                            let case_line = self.peek_line();
                            let (case_attributes, _) = self.parse_tag_attributes(case_line)?;
                            let mut body = self.parse_template_body(&["case"])?;
                            self.skip_closing("case");
                            // Template cases do not fall through.
                            body.push(Statement::new(StatementKind::Break(None), case_line));
                            let attribute = |key: &str| case_attributes.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
                            let value = attribute("value").ok_or_else(|| anyhow::anyhow!("<bx:case> requires a value attribute"))?;
                            // value="a,b" with delimiters="," matches any of the listed values.
                            let delimiters = attribute("delimiters").and_then(|d| literal_text(&d));
                            match (delimiters, literal_text(&value)) {
                                (Some(delimiters), Some(list)) => {
                                    for item in list.split(|c| delimiters.contains(c)) {
                                        let item = Expression::new(
                                            ExpressionKind::Literal(Literal::String(vec![StringPart::Text(item.trim().to_string())])),
                                            case_line,
                                        );
                                        cases.push(SwitchCase { value: number_literal(item), body: body.clone() });
                                    }
                                }
                                _ => cases.push(SwitchCase { value: number_literal(value), body }),
                            }
                        }
                        "defaultcase" => {
                            let default_line = self.peek_line();
                            self.parse_tag_attributes(default_line)?;
                            default_case = Some(self.parse_template_body(&["defaultcase"])?);
                            self.skip_closing("defaultcase");
                        }
                        other => anyhow::bail!("Unexpected <bx:{other}> inside <bx:switch>"),
                    }
                }
                _ => break,
            }
        }
        if !self_closing {
            self.skip_closing("switch");
        }
        Ok(Some(Statement::new(StatementKind::Switch {
            value: number_literal(value),
            cases, default_case,
        }, line)))
    }

    fn parse_include_tag(&mut self) -> Result<Option<Statement>> {
        // <bx:include template="part.bxm">
        let line = self.peek_line();
        let (attributes, self_closing) = self.parse_tag_attributes(line)?;
        let template = attributes
            .into_iter()
            .find(|(key, _)| key == "template")
            .map(|(_, value)| value)
            .ok_or_else(|| anyhow::anyhow!("<bx:include> requires a template attribute"))?;
        if !self_closing {
            self.skip_closing("include");
        }
        Ok(Some(Statement::new(StatementKind::Include(template), line)))
    }

    fn parse_import_tag(&mut self) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        Ok(Some(Statement::new(StatementKind::Import { path: String::new(), alias: None }, line)))
    }

    fn parse_throw_tag(&mut self) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        Ok(Some(Statement::new(StatementKind::Throw(None), line)))
    }

    fn parse_simple_tag(&mut self, kind: StatementKind) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        Ok(Some(Statement::new(kind, line)))
    }

    fn parse_function_tag(&mut self) -> Result<Option<Statement>> {
        let line = self.peek_line();
        self.skip_to_close();
        let body = self.parse_template_body(&["function"])?;
        self.skip_closing("function");
        Ok(Some(Statement::new(StatementKind::FunctionDecl {
            name: String::new(),
            attributes: vec![],
            modifiers: FunctionModifiers::default(),
            return_type: None,
            params: vec![],
            body: FunctionBody::Block(body),
        }, line)))
    }

    fn collect_text_until(&mut self, end_kind: TokenKind) -> String {
        let mut text = String::new();
        while self.pos < self.tokens.len() && self.peek_kind() != Some(end_kind) {
            if let Some(lexeme) = self.advance_lexeme() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&lexeme);
            } else {
                break;
            }
        }
        text
    }

    fn skip_expected(&mut self, kind: TokenKind) {
        if self.peek_kind() == Some(kind) {
            self.pos += 1;
        }
    }

    fn is_closing_tag(&self) -> bool {
        self.tokens.get(self.pos).is_some_and(|token| self.source[..token.span.start].rsplit_once('<').is_some_and(|(_, prefix)| prefix.starts_with('/')))
    }

    fn skip_closing(&mut self, name: &str) {
        if self.peek_kind() == Some(TokenKind::ComponentName)
            && self.peek_lexeme().is_some_and(|tag| tag.eq_ignore_ascii_case(name))
        {
            self.pos += 1;
            self.skip_to_close();
        }
    }
    fn skip_to_close(&mut self) {
        while self.pos < self.tokens.len() {
            match self.peek_kind() {
                Some(TokenKind::ComponentClose) | Some(TokenKind::ComponentSelfClose) => {
                    self.pos += 1;
                    return;
                }
                _ => { self.pos += 1; }
            }
        }
    }

}

pub(super) fn component_statement(name: &str, attributes: Vec<(Expression, Expression)>, body: Option<Vec<Statement>>, line: u32) -> Result<Statement> {
    Ok(Statement::new(StatementKind::Component { name: name.into(), attributes, body }, line))
}

/// The text of a plain string literal such as `"i"` in `index="i"`.
fn literal_text(expression: &Expression) -> Option<String> {
    match &expression.kind {
        ExpressionKind::Literal(Literal::String(parts)) => match parts.as_slice() {
            [StringPart::Text(text)] => Some(text.clone()),
            [] => Some(String::new()),
            _ => None,
        },
        _ => None,
    }
}

/// `from="1"` is the number 1; interpolated or computed values pass through.
fn number_literal(expression: Expression) -> Expression {
    match literal_text(&expression).and_then(|text| text.trim().parse::<f64>().ok()) {
        Some(number) => Expression::new(ExpressionKind::Literal(Literal::Number(number)), expression.line),
        None => expression,
    }
}

fn script_expression(text: &str, filename: Option<&str>) -> Result<Expression> {
    let ast = crate::parser::parse(&format!("{text};"), filename)?;
    match ast.into_iter().next() {
        Some(Statement { kind: StatementKind::Expression(expression), .. }) => Ok(expression),
        _ => anyhow::bail!("Invalid expression: {text}"),
    }
}

fn decode_template_text(text: &str) -> String {
    text.replace("##", "#")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_literal_text() {
        let ast = parse_template("hello", None).unwrap();
        assert_eq!(ast.len(), 1);
        assert!(matches!(ast[0].kind, StatementKind::BufferOutput(_)));
    }

    #[test]
    fn parse_set_tag() {
        let ast = parse_template("<bx:set x = 10>", None).unwrap();
        assert_eq!(ast.len(), 1);
        assert!(matches!(ast[0].kind, StatementKind::Expression(_)));
    }

    #[test]
    fn parse_if_tag() {
        let ast = parse_template("<bx:if x GT 5>big<bx:else>small</bx:if>", None).unwrap();
        assert_eq!(ast.len(), 1);
        assert!(matches!(ast[0].kind, StatementKind::If { .. }));
    }

    #[test]
    fn parse_if_elseif_else() {
        let ast = parse_template("<bx:if x GT 10>big<bx:elseif x GT 5>med<bx:else>small</bx:if>", None).unwrap();
        assert_eq!(ast.len(), 1);
    }
}
