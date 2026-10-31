use crate::datasource::traits::{QueryColumn, QueryColumnType, QueryResult, SqlValue};
use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriviaKind {
    Whitespace,
    LineComment,
    BlockComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Identifiers and literals
    Identifier,
    Number,
    String,
    OdbcDateTimeLiteral,
    BindParameter,

    // Keywords
    Select,
    From,
    Where,
    Group,
    By,
    Having,
    Order,
    Asc,
    Desc,
    Limit,
    Distinct,
    Union,
    All,
    As,
    And,
    Or,
    Not,
    In,
    Between,
    Like,
    Is,
    Null,
    Case,
    When,
    Then,
    Else,
    End,
    Cast,
    Convert,
    On,
    Join,
    Inner,
    Left,
    Right,
    Full,
    Outer,
    Cross,
    True,
    False,
    Contains,
    InstanceOf,
    CastAs,
    Xor,
    Eqv,

    // Punctuation and operators
    Comma,
    Dot,
    LeftParen,
    RightParen,
    Asterisk,
    Plus,
    Minus,
    Slash,
    Percent,
    Equal,
    Less,
    Greater,
    Bang,
    Colon,
    Semicolon,
    Question,
    LessEqual,
    GreaterEqual,
    EqualEqual,
    BangEqual,
    NotEqualAngle,
    PipePipe,
    AmpAmp,
    Ampersand,
    QuestionDot,
    QuestionColon,
    ColonColon,
    DotDot,
    DotDotLess,
    GreaterDotDot,
    GreaterDotDotLess,
    Caret,

    Eof,
}

#[derive(Debug, Clone)]
pub struct LexedSource<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    trivia: Vec<Trivia>,
}

impl<'a> LexedSource<'a> {
    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn tokens(&self) -> &[Token] {
        &self.tokens
    }

    pub fn trivia(&self) -> &[Trivia] {
        &self.trivia
    }

    pub fn into_parts(self) -> (&'a str, Vec<Token>, Vec<Trivia>) {
        (self.source, self.tokens, self.trivia)
    }

    pub fn text(&self, span: Span) -> &'a str {
        &self.source[span.start..span.end]
    }
}

pub fn lex(source: &str) -> LexedSource<'_> {
    Lexer::new(source).lex()
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<Token>,
    trivia: Vec<Trivia>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            trivia: Vec::new(),
        }
    }

    fn lex(mut self) -> LexedSource<'a> {
        while self.pos < self.source.len() {
            self.skip_trivia();
            if self.pos >= self.source.len() {
                break;
            }
            let start = self.pos;
            let line = self.line;
            let col = self.col;
            let kind = self.next_token_kind();
            self.tokens.push(Token {
                kind,
                span: Span {
                    start,
                    end: self.pos,
                    line,
                    col,
                },
            });
        }
        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: Span {
                start: self.pos,
                end: self.pos,
                line: self.line,
                col: self.col,
            },
        });
        LexedSource {
            source: self.source,
            tokens: self.tokens,
            trivia: self.trivia,
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            let start = self.pos;
            let line = self.line;
            let col = self.col;
            let Some(ch) = self.current_char() else {
                return;
            };

            if ch.is_whitespace() {
                self.consume_while(|c| c.is_whitespace());
                self.trivia.push(Trivia {
                    kind: TriviaKind::Whitespace,
                    span: Span {
                        start,
                        end: self.pos,
                        line,
                        col,
                    },
                });
                continue;
            }

            if self.source[self.pos..].starts_with("--") {
                self.advance();
                self.advance();
                while let Some(c) = self.current_char() {
                    if c == '\n' || c == '\r' {
                        break;
                    }
                    self.advance();
                }
                self.trivia.push(Trivia {
                    kind: TriviaKind::LineComment,
                    span: Span {
                        start,
                        end: self.pos,
                        line,
                        col,
                    },
                });
                continue;
            }

            if self.source[self.pos..].starts_with("/*") {
                self.advance();
                self.advance();
                while self.pos < self.source.len() && !self.source[self.pos..].starts_with("*/") {
                    self.advance();
                }
                if self.source[self.pos..].starts_with("*/") {
                    self.advance();
                    self.advance();
                }
                self.trivia.push(Trivia {
                    kind: TriviaKind::BlockComment,
                    span: Span {
                        start,
                        end: self.pos,
                        line,
                        col,
                    },
                });
                continue;
            }

            break;
        }
    }

    fn next_token_kind(&mut self) -> TokenKind {
        if let Some(kind) = self.lex_odbc_datetime_literal() {
            return kind;
        }

        let ch = self.current_char().unwrap_or('\0');
        if ch == '?' {
            self.advance();
            return TokenKind::BindParameter;
        }
        if ch == ':' {
            if self.peek_ident_start(1) {
                self.advance();
                self.consume_identifier();
                return TokenKind::BindParameter;
            }
        }
        if ch == '\'' {
            self.consume_string();
            return TokenKind::String;
        }
        if ch.is_ascii_digit()
            || (ch == '.' && self.peek_char(1).is_some_and(|c| c.is_ascii_digit()))
        {
            self.consume_number();
            return TokenKind::Number;
        }
        if self.is_ident_start(ch) || ch == '"' || ch == '`' || ch == '[' {
            let ident = self.consume_identifier_text();
            return keyword_or_ident(ident);
        }

        self.consume_operator_or_punct()
    }

    fn lex_odbc_datetime_literal(&mut self) -> Option<TokenKind> {
        let rest = &self.source[self.pos..];
        let lower = rest.to_ascii_lowercase();
        let kind = if lower.starts_with("{ts '") {
            Some("{ts '")
        } else if lower.starts_with("{d '") {
            Some("{d '")
        } else if lower.starts_with("{t '") {
            Some("{t '")
        } else {
            None
        }?;

        self.advance_n(kind.len());
        while self.pos < self.source.len() {
            if self.current_char() == Some('\'') && self.peek_char(1) == Some('}') {
                self.advance();
                self.advance();
                return Some(TokenKind::OdbcDateTimeLiteral);
            }
            self.advance();
        }
        Some(TokenKind::OdbcDateTimeLiteral)
    }

    fn consume_operator_or_punct(&mut self) -> TokenKind {
        let ch = self.current_char().unwrap_or('\0');
        match ch {
            ',' => {
                self.advance();
                TokenKind::Comma
            }
            '.' => {
                if self.peek_char(1) == Some('.') {
                    self.advance();
                    self.advance();
                    if self.current_char() == Some('<') {
                        self.advance();
                        TokenKind::DotDotLess
                    } else {
                        TokenKind::DotDot
                    }
                } else {
                    self.advance();
                    TokenKind::Dot
                }
            }
            '(' => {
                self.advance();
                TokenKind::LeftParen
            }
            ')' => {
                self.advance();
                TokenKind::RightParen
            }
            '*' => {
                self.advance();
                TokenKind::Asterisk
            }
            '+' => {
                self.advance();
                TokenKind::Plus
            }
            '-' => {
                self.advance();
                TokenKind::Minus
            }
            '/' => {
                self.advance();
                TokenKind::Slash
            }
            '%' => {
                self.advance();
                TokenKind::Percent
            }
            '^' => {
                self.advance();
                TokenKind::Caret
            }
            ';' => {
                self.advance();
                TokenKind::Semicolon
            }
            ':' => {
                if self.peek_char(1) == Some(':') {
                    self.advance();
                    self.advance();
                    TokenKind::ColonColon
                } else {
                    self.advance();
                    TokenKind::Colon
                }
            }
            '?' => {
                if self.peek_char(1) == Some('.') {
                    self.advance();
                    self.advance();
                    TokenKind::QuestionDot
                } else if self.peek_char(1) == Some(':') {
                    self.advance();
                    self.advance();
                    TokenKind::QuestionColon
                } else {
                    self.advance();
                    TokenKind::Question
                }
            }
            '=' => {
                if self.peek_char(1) == Some('=') {
                    self.advance();
                    self.advance();
                    TokenKind::EqualEqual
                } else if self.peek_char(1) == Some('>') {
                    self.advance();
                    self.advance();
                    TokenKind::Equal
                } else {
                    self.advance();
                    TokenKind::Equal
                }
            }
            '<' => {
                if self.peek_char(1) == Some('=') {
                    self.advance();
                    self.advance();
                    TokenKind::LessEqual
                } else if self.peek_char(1) == Some('>') {
                    self.advance();
                    self.advance();
                    TokenKind::NotEqualAngle
                } else if self.peek_char(1) == Some('.') && self.peek_char(2) == Some('.') {
                    self.advance();
                    self.advance();
                    self.advance();
                    TokenKind::DotDotLess
                } else {
                    self.advance();
                    TokenKind::Less
                }
            }
            '>' => {
                if self.peek_char(1) == Some('=') {
                    self.advance();
                    self.advance();
                    TokenKind::GreaterEqual
                } else if self.peek_char(1) == Some('.') && self.peek_char(2) == Some('.') {
                    self.advance();
                    self.advance();
                    self.advance();
                    if self.current_char() == Some('<') {
                        self.advance();
                        TokenKind::GreaterDotDotLess
                    } else {
                        TokenKind::GreaterDotDot
                    }
                } else {
                    self.advance();
                    TokenKind::Greater
                }
            }
            '!' => {
                if self.peek_char(1) == Some('=') {
                    self.advance();
                    self.advance();
                    TokenKind::BangEqual
                } else {
                    self.advance();
                    TokenKind::Bang
                }
            }
            '|' => {
                if self.peek_char(1) == Some('|') {
                    self.advance();
                    self.advance();
                    TokenKind::PipePipe
                } else {
                    self.advance();
                    TokenKind::PipePipe
                }
            }
            '&' => {
                if self.peek_char(1) == Some('&') {
                    self.advance();
                    self.advance();
                    TokenKind::AmpAmp
                } else {
                    self.advance();
                    TokenKind::Ampersand
                }
            }
            _ => {
                self.advance();
                TokenKind::Identifier
            }
        }
    }

    fn consume_identifier_text(&mut self) -> &'a str {
        let start = self.pos;
        if matches!(self.current_char(), Some('"') | Some('`') | Some('[')) {
            let quote = self.current_char().unwrap();
            self.advance();
            while let Some(ch) = self.current_char() {
                match quote {
                    '"' if ch == '"' && self.peek_char(1) != Some('"') => {
                        self.advance();
                        break;
                    }
                    '`' if ch == '`' && self.peek_char(1) != Some('`') => {
                        self.advance();
                        break;
                    }
                    '[' if ch == ']' => {
                        self.advance();
                        break;
                    }
                    _ => self.advance(),
                }
            }
            return &self.source[start..self.pos];
        }

        self.consume_identifier();
        &self.source[start..self.pos]
    }

    fn consume_identifier(&mut self) {
        self.advance();
        while let Some(ch) = self.current_char() {
            if self.is_ident_continue(ch) {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn consume_number(&mut self) {
        if self.current_char() == Some('.') {
            self.advance();
        }
        if self.current_char() == Some('0') && matches!(self.peek_char(1), Some('x' | 'X')) {
            self.advance();
            self.advance();
            while let Some(ch) = self.current_char() {
                if ch.is_ascii_hexdigit() {
                    self.advance();
                } else {
                    break;
                }
            }
            return;
        }
        while let Some(ch) = self.current_char() {
            if ch.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }
        if self.current_char() == Some('.') && self.peek_char(1).is_some_and(|c| c.is_ascii_digit())
        {
            self.advance();
            while let Some(ch) = self.current_char() {
                if ch.is_ascii_digit() {
                    self.advance();
                } else {
                    break;
                }
            }
        }
        if matches!(self.current_char(), Some('e' | 'E')) {
            let save = self.pos;
            self.advance();
            if matches!(self.current_char(), Some('+' | '-')) {
                self.advance();
            }
            let mut saw_digit = false;
            while let Some(ch) = self.current_char() {
                if ch.is_ascii_digit() {
                    saw_digit = true;
                    self.advance();
                } else {
                    break;
                }
            }
            if !saw_digit {
                self.pos = save;
            }
        }
    }

    fn consume_string(&mut self) {
        self.advance();
        while let Some(ch) = self.current_char() {
            self.advance();
            if ch == '\'' {
                if self.current_char() == Some('\'') {
                    self.advance();
                    continue;
                }
                break;
            }
        }
    }

    fn consume_while<F>(&mut self, mut predicate: F)
    where
        F: FnMut(char) -> bool,
    {
        while let Some(ch) = self.current_char() {
            if predicate(ch) {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn advance(&mut self) {
        if let Some(ch) = self.current_char() {
            self.pos += ch.len_utf8();
            if ch == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
    }

    fn advance_n(&mut self, n: usize) {
        for _ in 0..n {
            self.advance();
        }
    }

    fn current_char(&self) -> Option<char> {
        self.source[self.pos..].chars().next()
    }

    fn peek_char(&self, offset: usize) -> Option<char> {
        self.source[self.pos..].chars().nth(offset)
    }

    fn peek_ident_start(&self, offset: usize) -> bool {
        self.peek_char(offset)
            .is_some_and(|ch| self.is_ident_start(ch))
    }

    fn is_ident_start(&self, ch: char) -> bool {
        ch.is_ascii_alphabetic() || ch == '_' || ch == '$'
    }

    fn is_ident_continue(&self, ch: char) -> bool {
        ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
    }
}

fn keyword_or_ident(lexeme: &str) -> TokenKind {
    match lexeme.to_ascii_lowercase().as_str() {
        "select" => TokenKind::Select,
        "from" => TokenKind::From,
        "where" => TokenKind::Where,
        "group" => TokenKind::Group,
        "by" => TokenKind::By,
        "having" => TokenKind::Having,
        "order" => TokenKind::Order,
        "asc" => TokenKind::Asc,
        "desc" => TokenKind::Desc,
        "limit" => TokenKind::Limit,
        "distinct" => TokenKind::Distinct,
        "union" => TokenKind::Union,
        "all" => TokenKind::All,
        "as" => TokenKind::As,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "not" => TokenKind::Not,
        "in" => TokenKind::In,
        "between" => TokenKind::Between,
        "like" => TokenKind::Like,
        "is" => TokenKind::Is,
        "null" => TokenKind::Null,
        "case" => TokenKind::Case,
        "when" => TokenKind::When,
        "then" => TokenKind::Then,
        "else" => TokenKind::Else,
        "end" => TokenKind::End,
        "cast" => TokenKind::Cast,
        "convert" => TokenKind::Convert,
        "on" => TokenKind::On,
        "join" => TokenKind::Join,
        "inner" => TokenKind::Inner,
        "left" => TokenKind::Left,
        "right" => TokenKind::Right,
        "full" => TokenKind::Full,
        "outer" => TokenKind::Outer,
        "cross" => TokenKind::Cross,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "contains" => TokenKind::Contains,
        "instanceof" => TokenKind::InstanceOf,
        "castas" => TokenKind::CastAs,
        "xor" => TokenKind::Xor,
        "eqv" => TokenKind::Eqv,
        _ => TokenKind::Identifier,
    }
}

pub type ParseResult<T> = Result<T, ParseError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: String,
    pub span: Span,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, col {}",
            self.message, self.span.line, self.span.col
        )
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub kind: QueryKind,
    pub span: Span,
}

impl Query {
    pub fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::Query(self));
        match &self.kind {
            QueryKind::Select(select) => select.walk(f),
            QueryKind::Union { left, right, .. } => {
                left.walk(f);
                right.walk(f);
            }
        }
    }

    pub fn expressions<'a>(&'a self) -> Vec<&'a Expression> {
        let mut out = Vec::new();
        self.collect_expressions(&mut out);
        out
    }

    pub fn tables<'a>(&'a self) -> Vec<&'a TableRef> {
        let mut out = Vec::new();
        self.collect_tables(&mut out);
        out
    }

    fn collect_expressions<'a>(&'a self, out: &mut Vec<&'a Expression>) {
        match &self.kind {
            QueryKind::Select(select) => select.collect_expressions(out),
            QueryKind::Union { left, right, .. } => {
                left.collect_expressions(out);
                right.collect_expressions(out);
            }
        }
    }

    fn collect_tables<'a>(&'a self, out: &mut Vec<&'a TableRef>) {
        match &self.kind {
            QueryKind::Select(select) => select.collect_tables(out),
            QueryKind::Union { left, right, .. } => {
                left.collect_tables(out);
                right.collect_tables(out);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum QueryKind {
    Select(SelectStatement),
    Union {
        left: Box<Query>,
        all: bool,
        right: Box<Query>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectStatement {
    pub distinct: bool,
    pub projection: Vec<SelectItem>,
    pub from: Vec<TableRef>,
    pub where_clause: Option<Expression>,
    pub group_by: Vec<Expression>,
    pub having: Option<Expression>,
    pub order_by: Vec<OrderByItem>,
    pub limit: Option<u64>,
}

impl SelectStatement {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::SelectStatement(self));
        for item in &self.projection {
            item.walk(f);
        }
        for table in &self.from {
            table.walk(f);
        }
        if let Some(expr) = &self.where_clause {
            expr.walk(f);
        }
        for expr in &self.group_by {
            expr.walk(f);
        }
        if let Some(expr) = &self.having {
            expr.walk(f);
        }
        for item in &self.order_by {
            item.walk(f);
        }
    }

    fn collect_expressions<'a>(&'a self, out: &mut Vec<&'a Expression>) {
        for item in &self.projection {
            out.push(&item.expr);
        }
        for table in &self.from {
            table.collect_expressions(out);
        }
        if let Some(expr) = &self.where_clause {
            out.push(expr);
        }
        for expr in &self.group_by {
            out.push(expr);
        }
        if let Some(expr) = &self.having {
            out.push(expr);
        }
        for item in &self.order_by {
            out.push(&item.expr);
        }
    }

    fn collect_tables<'a>(&'a self, out: &mut Vec<&'a TableRef>) {
        for table in &self.from {
            table.collect_tables(out);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectItem {
    pub expr: Expression,
    pub alias: Option<String>,
}

impl SelectItem {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::SelectItem(self));
        self.expr.walk(f);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRef {
    pub source: TableSource,
    pub alias: Option<String>,
    pub joins: Vec<JoinClause>,
}

impl TableRef {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::TableRef(self));
        if let TableSource::Subquery(query) = &self.source {
            query.walk(f);
        }
        for join in &self.joins {
            join.walk(f);
        }
    }

    fn collect_expressions<'a>(&'a self, out: &mut Vec<&'a Expression>) {
        if let TableSource::Subquery(query) = &self.source {
            query.collect_expressions(out);
        }
        for join in &self.joins {
            join.collect_expressions(out);
        }
    }

    fn collect_tables<'a>(&'a self, out: &mut Vec<&'a TableRef>) {
        out.push(self);
        if let TableSource::Subquery(query) = &self.source {
            query.collect_tables(out);
        }
        for join in &self.joins {
            join.collect_tables(out);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableSource {
    Named(Vec<String>),
    Subquery(Box<Query>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct JoinClause {
    pub kind: JoinKind,
    pub table: TableRef,
    pub on: Option<Expression>,
}

impl JoinClause {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::JoinClause(self));
        self.table.walk(f);
        if let Some(expr) = &self.on {
            expr.walk(f);
        }
    }

    fn collect_expressions<'a>(&'a self, out: &mut Vec<&'a Expression>) {
        self.table.collect_expressions(out);
        if let Some(expr) = &self.on {
            out.push(expr);
        }
    }

    fn collect_tables<'a>(&'a self, out: &mut Vec<&'a TableRef>) {
        self.table.collect_tables(out);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderByItem {
    pub expr: Expression,
    pub descending: bool,
}

impl OrderByItem {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::OrderByItem(self));
        self.expr.walk(f);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Identifier(Vec<String>),
    String(String),
    Number(String),
    Boolean(bool),
    Null,
    OdbcDateTime(String),
    BindParameter(Option<String>),
    Star,
    QualifiedStar(Vec<String>),
    FunctionCall {
        name: Vec<String>,
        args: Vec<Expression>,
    },
    Subquery(Box<Query>),
    Case {
        branches: Vec<(Expression, Expression)>,
        else_expr: Option<Box<Expression>>,
    },
    Paren(Box<Expression>),
    Unary {
        op: UnaryOp,
        expr: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    IsNull {
        expr: Box<Expression>,
        negated: bool,
    },
}

impl Expression {
    fn walk(&self, f: &mut impl FnMut(QueryNode<'_>)) {
        f(QueryNode::Expression(self));
        match self {
            Expression::FunctionCall { args, .. } => {
                for arg in args {
                    arg.walk(f);
                }
            }
            Expression::Subquery(query) => query.walk(f),
            Expression::Case {
                branches,
                else_expr,
            } => {
                for (cond, value) in branches {
                    cond.walk(f);
                    value.walk(f);
                }
                if let Some(expr) = else_expr {
                    expr.walk(f);
                }
            }
            Expression::Paren(inner)
            | Expression::Unary { expr: inner, .. }
            | Expression::IsNull { expr: inner, .. } => inner.walk(f),
            Expression::Binary { left, right, .. } => {
                left.walk(f);
                right.walk(f);
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceColumnDependencyPlan {
    pub sources: Vec<SourceColumnDependency>,
    pub safe_for_pruning: bool,
}

impl SourceColumnDependencyPlan {
    fn for_select(select: &SelectStatement) -> Self {
        let mut planner = SourceColumnPlanner::default();
        planner.collect_select(select);
        planner.finish()
    }

    fn source_for(&self, source_path: &[String], alias: &str) -> Option<&SourceColumnDependency> {
        self.sources.iter().find(|source| {
            path_eq(&source.source_path, source_path) && source.alias.eq_ignore_ascii_case(alias)
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceColumnDependency {
    pub source_path: Vec<String>,
    pub alias: String,
    pub all_columns_required: bool,
    pub columns: Vec<SourceColumnRequirement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceColumnRequirement {
    pub name: String,
    pub usages: Vec<SourceColumnUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceColumnUsage {
    Projection,
    Where,
    GroupBy,
    Having,
    OrderBy,
    Aggregate,
    JoinOn,
}

pub fn source_column_dependency_plan(query: &Query) -> SourceColumnDependencyPlan {
    let mut planner = SourceColumnPlanner::default();
    planner.collect_query(query);
    planner.finish()
}

struct SourceColumnPlanner {
    sources: Vec<SourceColumnDependencyBuilder>,
    safe_for_pruning: bool,
}

impl Default for SourceColumnPlanner {
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            safe_for_pruning: true,
        }
    }
}

impl SourceColumnPlanner {
    fn collect_query(&mut self, query: &Query) {
        match &query.kind {
            QueryKind::Select(select) => self.collect_select(select),
            QueryKind::Union { left, right, .. } => {
                self.collect_query(left);
                self.collect_query(right);
            }
        }
    }

    fn collect_select(&mut self, select: &SelectStatement) {
        let mut bindings = Vec::new();
        for table in &select.from {
            self.collect_table_bindings(table, &mut bindings);
        }

        for table in &select.from {
            self.collect_join_dependencies(table, &bindings);
        }
        for item in &select.projection {
            self.collect_expr(&item.expr, SourceColumnUsage::Projection, &bindings);
        }
        if let Some(expr) = &select.where_clause {
            self.collect_expr(expr, SourceColumnUsage::Where, &bindings);
        }
        for expr in &select.group_by {
            self.collect_expr(expr, SourceColumnUsage::GroupBy, &bindings);
        }
        if let Some(expr) = &select.having {
            self.collect_expr(expr, SourceColumnUsage::Having, &bindings);
        }
        for item in &select.order_by {
            self.collect_expr(&item.expr, SourceColumnUsage::OrderBy, &bindings);
        }
    }

    fn collect_table_bindings(&mut self, table: &TableRef, bindings: &mut Vec<SourceBinding>) {
        match &table.source {
            TableSource::Named(path) => {
                let alias = table
                    .alias
                    .clone()
                    .or_else(|| path.last().cloned())
                    .unwrap_or_else(|| "query".to_string());
                let source_idx = self.ensure_source(path, &alias);
                bindings.push(SourceBinding {
                    source_idx,
                    source_path: path.clone(),
                    alias,
                });
            }
            TableSource::Subquery(query) => self.collect_query(query),
        }

        for join in &table.joins {
            self.collect_table_bindings(&join.table, bindings);
        }
    }

    fn collect_join_dependencies(&mut self, table: &TableRef, bindings: &[SourceBinding]) {
        for join in &table.joins {
            if let Some(expr) = &join.on {
                self.collect_expr(expr, SourceColumnUsage::JoinOn, bindings);
            }
            self.collect_join_dependencies(&join.table, bindings);
        }
    }

    fn collect_expr(
        &mut self,
        expr: &Expression,
        usage: SourceColumnUsage,
        bindings: &[SourceBinding],
    ) {
        match expr {
            Expression::Identifier(path) => self.record_identifier(path, usage, bindings),
            Expression::Star => self.record_all_columns(bindings),
            Expression::QualifiedStar(path) => self.record_qualified_star(path, bindings),
            Expression::FunctionCall { name, args } => {
                let aggregate = is_aggregate_name(name);
                for arg in args {
                    if aggregate && is_count_name(name) && matches!(arg, Expression::Star) {
                        continue;
                    }
                    self.collect_expr(arg, usage, bindings);
                    if aggregate {
                        self.collect_expr(arg, SourceColumnUsage::Aggregate, bindings);
                    }
                }
            }
            Expression::Subquery(query) => self.collect_query(query),
            Expression::Case {
                branches,
                else_expr,
            } => {
                for (cond, value) in branches {
                    self.collect_expr(cond, usage, bindings);
                    self.collect_expr(value, usage, bindings);
                }
                if let Some(expr) = else_expr {
                    self.collect_expr(expr, usage, bindings);
                }
            }
            Expression::Paren(inner)
            | Expression::Unary { expr: inner, .. }
            | Expression::IsNull { expr: inner, .. } => self.collect_expr(inner, usage, bindings),
            Expression::Binary { left, right, .. } => {
                self.collect_expr(left, usage, bindings);
                self.collect_expr(right, usage, bindings);
            }
            Expression::String(_)
            | Expression::Number(_)
            | Expression::Boolean(_)
            | Expression::Null
            | Expression::OdbcDateTime(_)
            | Expression::BindParameter(_) => {}
        }
    }

    fn record_identifier(
        &mut self,
        path: &[String],
        usage: SourceColumnUsage,
        bindings: &[SourceBinding],
    ) {
        let Some((source_idx, column_name)) = resolve_identifier_binding(path, bindings) else {
            self.safe_for_pruning = false;
            return;
        };
        self.sources[source_idx].record_column(column_name, usage);
    }

    fn record_all_columns(&mut self, bindings: &[SourceBinding]) {
        for binding in bindings {
            self.sources[binding.source_idx].all_columns_required = true;
        }
    }

    fn record_qualified_star(&mut self, path: &[String], bindings: &[SourceBinding]) {
        let matches = matching_bindings(path, bindings);
        if matches.len() != 1 {
            self.safe_for_pruning = false;
            self.record_all_columns(bindings);
            return;
        }
        self.sources[matches[0].source_idx].all_columns_required = true;
    }

    fn ensure_source(&mut self, source_path: &[String], alias: &str) -> usize {
        if let Some(idx) = self.sources.iter().position(|source| {
            path_eq(&source.source_path, source_path) && source.alias.eq_ignore_ascii_case(alias)
        }) {
            return idx;
        }

        let idx = self.sources.len();
        self.sources.push(SourceColumnDependencyBuilder {
            source_path: source_path.to_vec(),
            alias: alias.to_string(),
            all_columns_required: false,
            columns: HashMap::new(),
        });
        idx
    }

    fn finish(self) -> SourceColumnDependencyPlan {
        SourceColumnDependencyPlan {
            sources: self
                .sources
                .into_iter()
                .map(SourceColumnDependencyBuilder::finish)
                .collect(),
            safe_for_pruning: self.safe_for_pruning,
        }
    }
}

#[derive(Clone, Debug)]
struct SourceBinding {
    source_idx: usize,
    source_path: Vec<String>,
    alias: String,
}

#[derive(Debug)]
struct SourceColumnDependencyBuilder {
    source_path: Vec<String>,
    alias: String,
    all_columns_required: bool,
    columns: HashMap<String, SourceColumnRequirementBuilder>,
}

impl SourceColumnDependencyBuilder {
    fn record_column(&mut self, name: &str, usage: SourceColumnUsage) {
        self.columns
            .entry(name.to_lowercase())
            .or_insert_with(|| SourceColumnRequirementBuilder {
                name: name.to_string(),
                usages: BTreeSet::new(),
            })
            .usages
            .insert(usage);
    }

    fn finish(self) -> SourceColumnDependency {
        let mut columns: Vec<_> = self
            .columns
            .into_values()
            .map(SourceColumnRequirementBuilder::finish)
            .collect();
        columns.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

        SourceColumnDependency {
            source_path: self.source_path,
            alias: self.alias,
            all_columns_required: self.all_columns_required,
            columns,
        }
    }
}

#[derive(Debug)]
struct SourceColumnRequirementBuilder {
    name: String,
    usages: BTreeSet<SourceColumnUsage>,
}

impl SourceColumnRequirementBuilder {
    fn finish(self) -> SourceColumnRequirement {
        SourceColumnRequirement {
            name: self.name,
            usages: self.usages.into_iter().collect(),
        }
    }
}

fn resolve_identifier_binding<'a>(
    path: &'a [String],
    bindings: &'a [SourceBinding],
) -> Option<(usize, &'a str)> {
    match path {
        [column] if bindings.len() == 1 => Some((bindings[0].source_idx, column.as_str())),
        [_] => None,
        [.., column] => {
            let qualifier = &path[..path.len() - 1];
            let matches = matching_bindings(qualifier, bindings);
            if matches.len() == 1 {
                Some((matches[0].source_idx, column.as_str()))
            } else {
                None
            }
        }
        [] => None,
    }
}

fn matching_bindings<'a>(
    qualifier: &[String],
    bindings: &'a [SourceBinding],
) -> Vec<&'a SourceBinding> {
    bindings
        .iter()
        .filter(|binding| binding.matches_qualifier(qualifier))
        .collect()
}

impl SourceBinding {
    fn matches_qualifier(&self, qualifier: &[String]) -> bool {
        if qualifier.len() == 1 && self.alias.eq_ignore_ascii_case(&qualifier[0]) {
            return true;
        }
        if qualifier.len() == 1
            && self
                .source_path
                .last()
                .is_some_and(|name| name.eq_ignore_ascii_case(&qualifier[0]))
        {
            return true;
        }
        path_eq(&self.source_path, qualifier)
    }
}

fn path_eq(left: &[String], right: &[String]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right.iter())
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QueryNode<'a> {
    Query(&'a Query),
    SelectStatement(&'a SelectStatement),
    SelectItem(&'a SelectItem),
    TableRef(&'a TableRef),
    JoinClause(&'a JoinClause),
    OrderByItem(&'a OrderByItem),
    Expression(&'a Expression),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnaryOp {
    Not,
    Negate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    Like,
    Contains,
    InstanceOf,
    CastAs,
    Xor,
    Eqv,
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Concat,
    Pow,
}

pub fn parse(source: &str) -> ParseResult<Query> {
    let lexed = lex(source);
    Parser::new(source, lexed.tokens()).parse_query()
}

struct Parser<'a> {
    source: &'a str,
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str, tokens: &'a [Token]) -> Self {
        Self {
            source,
            tokens,
            pos: 0,
        }
    }

    fn parse_query(&mut self) -> ParseResult<Query> {
        let query = self.parse_query_inner()?;
        self.expect_eof()?;
        Ok(query)
    }

    fn parse_query_inner(&mut self) -> ParseResult<Query> {
        let mut query = self.parse_select_query()?;
        while self.peek_is(TokenKind::Union) {
            let start = query.span;
            self.pos += 1;
            let all = self.peek_is(TokenKind::All);
            if all {
                self.pos += 1;
            }
            let right = self.parse_select_query()?;
            let span = self.merge_spans(start, right.span);
            query = Query {
                span,
                kind: QueryKind::Union {
                    left: Box::new(query),
                    all,
                    right: Box::new(right),
                },
            };
        }
        Ok(query)
    }

    fn parse_select_query(&mut self) -> ParseResult<Query> {
        let start = self.expect_kind(TokenKind::Select)?;
        let distinct = self.consume_if(TokenKind::Distinct);

        let mut projection = Vec::new();
        loop {
            projection.push(self.parse_select_item()?);
            if !self.consume_if(TokenKind::Comma) {
                break;
            }
        }

        let mut from = Vec::new();
        if self.consume_if(TokenKind::From) {
            loop {
                from.push(self.parse_table_ref()?);
                if !self.consume_if(TokenKind::Comma) {
                    break;
                }
            }
        }

        let where_clause = if self.consume_if(TokenKind::Where) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        let mut group_by = Vec::new();
        if self.consume_if(TokenKind::Group) {
            self.expect_kind(TokenKind::By)?;
            loop {
                group_by.push(self.parse_expression()?);
                if !self.consume_if(TokenKind::Comma) {
                    break;
                }
            }
        }

        let having = if self.consume_if(TokenKind::Having) {
            Some(self.parse_expression()?)
        } else {
            None
        };

        let mut order_by = Vec::new();
        if self.consume_if(TokenKind::Order) {
            self.expect_kind(TokenKind::By)?;
            loop {
                let expr = self.parse_expression()?;
                let descending = self.consume_if(TokenKind::Desc);
                let _ascending = self.consume_if(TokenKind::Asc);
                order_by.push(OrderByItem { expr, descending });
                if !self.consume_if(TokenKind::Comma) {
                    break;
                }
            }
        }

        let limit = if self.consume_if(TokenKind::Limit) {
            let token = self.expect_any(&[TokenKind::Number])?;
            let raw = self.token_text(token);
            Some(
                raw.parse::<u64>()
                    .map_err(|_| self.error(token, "invalid LIMIT value"))?,
            )
        } else {
            None
        };

        let end = self.prev_span().unwrap_or(start);
        Ok(Query {
            span: self.merge_spans(start, end),
            kind: QueryKind::Select(SelectStatement {
                distinct,
                projection,
                from,
                where_clause,
                group_by,
                having,
                order_by,
                limit,
            }),
        })
    }

    fn parse_select_item(&mut self) -> ParseResult<SelectItem> {
        let expr = if self.consume_if(TokenKind::Asterisk) {
            Expression::Star
        } else if self.peek_is(TokenKind::Identifier)
            && self.peek_kind_n(1) == Some(TokenKind::Dot)
            && self.peek_kind_n(2) == Some(TokenKind::Asterisk)
        {
            let path = self.parse_path()?;
            self.expect_kind(TokenKind::Dot)?;
            self.expect_kind(TokenKind::Asterisk)?;
            Expression::QualifiedStar(path)
        } else {
            self.parse_expression()?
        };

        let alias = if self.consume_if(TokenKind::As) {
            Some(self.expect_identifier_string()?)
        } else {
            None
        };

        Ok(SelectItem { expr, alias })
    }

    fn parse_table_ref(&mut self) -> ParseResult<TableRef> {
        let source = if self.consume_if(TokenKind::LeftParen) {
            let subquery = self.parse_query_inner()?;
            self.expect_kind(TokenKind::RightParen)?;
            TableSource::Subquery(Box::new(subquery))
        } else {
            TableSource::Named(self.parse_path()?)
        };

        let alias = if self.consume_if(TokenKind::As) {
            Some(self.expect_identifier_string()?)
        } else if self.peek_is(TokenKind::Identifier) {
            Some(self.expect_identifier_string()?)
        } else {
            None
        };

        let mut joins = Vec::new();
        while self.peek_is(TokenKind::Join)
            || self.peek_is(TokenKind::Inner)
            || self.peek_is(TokenKind::Left)
            || self.peek_is(TokenKind::Right)
            || self.peek_is(TokenKind::Full)
            || self.peek_is(TokenKind::Cross)
        {
            joins.push(self.parse_join_clause()?);
        }

        Ok(TableRef {
            source,
            alias,
            joins,
        })
    }

    fn parse_join_clause(&mut self) -> ParseResult<JoinClause> {
        let kind = if self.consume_if(TokenKind::Inner) {
            let _ = self.consume_if(TokenKind::Join);
            JoinKind::Inner
        } else if self.consume_if(TokenKind::Left) {
            let _ = self.consume_if(TokenKind::Outer);
            self.expect_kind(TokenKind::Join)?;
            JoinKind::Left
        } else if self.consume_if(TokenKind::Right) {
            let _ = self.consume_if(TokenKind::Outer);
            self.expect_kind(TokenKind::Join)?;
            JoinKind::Right
        } else if self.consume_if(TokenKind::Full) {
            let _ = self.consume_if(TokenKind::Outer);
            self.expect_kind(TokenKind::Join)?;
            JoinKind::Full
        } else if self.consume_if(TokenKind::Cross) {
            self.expect_kind(TokenKind::Join)?;
            JoinKind::Cross
        } else {
            self.expect_kind(TokenKind::Join)?;
            JoinKind::Inner
        };

        let table = self.parse_table_ref()?;
        let on = if self.consume_if(TokenKind::On) {
            Some(self.parse_expression()?)
        } else {
            None
        };
        Ok(JoinClause { kind, table, on })
    }

    fn parse_expression(&mut self) -> ParseResult<Expression> {
        self.parse_binary_expression(0)
    }

    fn parse_binary_expression(&mut self, min_prec: u8) -> ParseResult<Expression> {
        let mut left = self.parse_prefix_expression()?;
        loop {
            if self.peek_is(TokenKind::Is) {
                let save = self.pos;
                self.pos += 1;
                let negated = self.consume_if(TokenKind::Not);
                if self.peek_is(TokenKind::Null) {
                    self.pos += 1;
                    left = Expression::IsNull {
                        expr: Box::new(left),
                        negated,
                    };
                    continue;
                }
                self.pos = save;
            }

            let Some((prec, op)) = self.peek_binary_op() else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.pos += self.binary_op_span_len();
            let right = self.parse_binary_expression(prec + 1)?;
            left = Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        Ok(left)
    }

    fn parse_prefix_expression(&mut self) -> ParseResult<Expression> {
        if self.consume_if(TokenKind::Not) {
            return Ok(Expression::Unary {
                op: UnaryOp::Not,
                expr: Box::new(self.parse_prefix_expression()?),
            });
        }
        if self.consume_if(TokenKind::Minus) {
            return Ok(Expression::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(self.parse_prefix_expression()?),
            });
        }

        let mut expr = self.parse_primary_expression()?;
        loop {
            if self.consume_if(TokenKind::LeftParen) {
                let mut args = Vec::new();
                if !self.consume_if(TokenKind::RightParen) {
                    loop {
                        args.push(self.parse_expression()?);
                        if self.consume_if(TokenKind::Comma) {
                            continue;
                        }
                        self.expect_kind(TokenKind::RightParen)?;
                        break;
                    }
                }
                expr = match expr {
                    Expression::Identifier(name) | Expression::QualifiedStar(name) => {
                        Expression::FunctionCall { name, args }
                    }
                    Expression::Paren(inner) => Expression::FunctionCall {
                        name: vec![self.text_for_expr_name(&inner)?],
                        args,
                    },
                    other => Expression::FunctionCall {
                        name: vec![self.expr_name(other)?],
                        args,
                    },
                };
                continue;
            }
            if self.consume_if(TokenKind::Dot) {
                let member = self.expect_identifier_string()?;
                expr = match expr {
                    Expression::Identifier(mut path) => {
                        path.push(member);
                        Expression::Identifier(path)
                    }
                    _ => Expression::Binary {
                        left: Box::new(expr),
                        op: BinaryOp::Concat,
                        right: Box::new(Expression::Identifier(vec![member])),
                    },
                };
                continue;
            }
            break;
        }
        Ok(expr)
    }

    fn parse_primary_expression(&mut self) -> ParseResult<Expression> {
        if self.consume_if(TokenKind::LeftParen) {
            if self.peek_is(TokenKind::Select) {
                let subquery = self.parse_query_inner()?;
                self.expect_kind(TokenKind::RightParen)?;
                return Ok(Expression::Subquery(Box::new(subquery)));
            }
            let expr = self.parse_expression()?;
            self.expect_kind(TokenKind::RightParen)?;
            return Ok(Expression::Paren(Box::new(expr)));
        }
        if self.consume_if(TokenKind::Case) {
            let mut branches = Vec::new();
            while self.consume_if(TokenKind::When) {
                let cond = self.parse_expression()?;
                self.expect_kind(TokenKind::Then)?;
                let then_expr = self.parse_expression()?;
                branches.push((cond, then_expr));
            }
            let else_expr = if self.consume_if(TokenKind::Else) {
                Some(Box::new(self.parse_expression()?))
            } else {
                None
            };
            self.expect_kind(TokenKind::End)?;
            return Ok(Expression::Case {
                branches,
                else_expr,
            });
        }
        if self.peek_is(TokenKind::String) {
            let raw = self.consume_raw_text()?;
            let value = raw
                .strip_prefix('\'')
                .and_then(|s| s.strip_suffix('\''))
                .ok_or_else(|| self.error_current("unterminated string literal"))?;
            return Ok(Expression::String(value.replace("''", "'")));
        }
        if self.peek_is(TokenKind::Number) {
            return Ok(Expression::Number(self.consume_raw_text()?));
        }
        if self.consume_if(TokenKind::True) {
            return Ok(Expression::Boolean(true));
        }
        if self.consume_if(TokenKind::False) {
            return Ok(Expression::Boolean(false));
        }
        if self.consume_if(TokenKind::Null) {
            return Ok(Expression::Null);
        }
        if self.peek_is(TokenKind::BindParameter) {
            let text = self.consume_raw_text()?;
            return Ok(Expression::BindParameter(
                text.strip_prefix(':').map(|s| s.to_string()),
            ));
        }
        if self.peek_is(TokenKind::OdbcDateTimeLiteral) {
            return Ok(Expression::OdbcDateTime(self.consume_raw_text()?));
        }
        if self.peek_is(TokenKind::Asterisk) {
            self.pos += 1;
            return Ok(Expression::Star);
        }
        if self.peek_is(TokenKind::Select) {
            let _subquery = self.parse_select_query()?;
            return Ok(Expression::Paren(Box::new(Expression::Identifier(vec![
                "subquery".to_string(),
            ]))));
        }
        if self.peek_is(TokenKind::Identifier) {
            let path = self.parse_path()?;
            if self.peek_is(TokenKind::LeftParen) {
                return Ok(Expression::Identifier(path));
            }
            return Ok(Expression::Identifier(path));
        }
        Err(self.error_current("expected expression"))
    }

    fn peek_binary_op(&self) -> Option<(u8, BinaryOp)> {
        match self.peek_kind()? {
            TokenKind::Or => Some((1, BinaryOp::Or)),
            TokenKind::Xor => Some((1, BinaryOp::Xor)),
            TokenKind::Eqv => Some((1, BinaryOp::Eqv)),
            TokenKind::AmpAmp | TokenKind::And => Some((2, BinaryOp::And)),
            TokenKind::EqualEqual | TokenKind::Equal => Some((3, BinaryOp::Eq)),
            TokenKind::BangEqual | TokenKind::NotEqualAngle => Some((3, BinaryOp::NotEq)),
            TokenKind::Less => Some((3, BinaryOp::Lt)),
            TokenKind::LessEqual => Some((3, BinaryOp::Lte)),
            TokenKind::Greater => Some((3, BinaryOp::Gt)),
            TokenKind::GreaterEqual => Some((3, BinaryOp::Gte)),
            TokenKind::Like => Some((3, BinaryOp::Like)),
            TokenKind::Contains => Some((3, BinaryOp::Contains)),
            TokenKind::InstanceOf => Some((3, BinaryOp::InstanceOf)),
            TokenKind::CastAs => Some((3, BinaryOp::CastAs)),
            TokenKind::Plus => Some((4, BinaryOp::Add)),
            TokenKind::Minus => Some((4, BinaryOp::Sub)),
            TokenKind::Asterisk => Some((5, BinaryOp::Mul)),
            TokenKind::Slash => Some((5, BinaryOp::Div)),
            TokenKind::Percent => Some((5, BinaryOp::Mod)),
            TokenKind::Ampersand => Some((6, BinaryOp::Concat)),
            TokenKind::Caret => Some((7, BinaryOp::Pow)),
            _ => None,
        }
    }

    fn binary_op_span_len(&self) -> usize {
        1
    }

    fn parse_path(&mut self) -> ParseResult<Vec<String>> {
        let mut path = vec![self.expect_identifier_string()?];
        while self.consume_if(TokenKind::Dot) {
            path.push(self.expect_identifier_string()?);
        }
        Ok(path)
    }

    fn expect_identifier_string(&mut self) -> ParseResult<String> {
        if self.peek_is(TokenKind::Identifier) {
            Ok(self.consume_raw_text()?)
        } else {
            Err(self.error_current("expected identifier"))
        }
    }

    fn consume_raw_text(&mut self) -> ParseResult<String> {
        let token = *self
            .current_token()
            .ok_or_else(|| self.error_eof("unexpected EOF"))?;
        self.pos += 1;
        Ok(self.token_text(token).to_string())
    }

    fn expect_kind(&mut self, kind: TokenKind) -> ParseResult<Span> {
        if self.peek_is(kind) {
            let span = self.current_span().unwrap();
            self.pos += 1;
            Ok(span)
        } else {
            Err(self.error_current(&format!("expected {:?}", kind)))
        }
    }

    fn expect_any(&mut self, kinds: &[TokenKind]) -> ParseResult<Token> {
        if kinds.iter().any(|kind| self.peek_is(*kind)) {
            let token = *self.current_token().unwrap();
            self.pos += 1;
            Ok(token)
        } else {
            Err(self.error_current("unexpected token"))
        }
    }

    fn consume_if(&mut self, kind: TokenKind) -> bool {
        if self.peek_is(kind) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn peek_is(&self, kind: TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn peek_kind(&self) -> Option<TokenKind> {
        self.current_token().map(|t| t.kind)
    }

    fn peek_kind_n(&self, n: usize) -> Option<TokenKind> {
        self.tokens.get(self.pos + n).map(|t| t.kind)
    }

    fn current_token(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn current_span(&self) -> Option<Span> {
        self.current_token().map(|t| t.span)
    }

    fn prev_span(&self) -> Option<Span> {
        self.tokens.get(self.pos.saturating_sub(1)).map(|t| t.span)
    }

    fn token_text(&self, token: Token) -> &str {
        self.source[token.span.start..token.span.end].as_ref()
    }

    fn merge_spans(&self, start: Span, end: Span) -> Span {
        Span {
            start: start.start,
            end: end.end,
            line: start.line,
            col: start.col,
        }
    }

    fn expect_eof(&mut self) -> ParseResult<()> {
        if self.peek_is(TokenKind::Eof) {
            Ok(())
        } else {
            Err(self.error_current("unexpected trailing tokens"))
        }
    }

    fn error_current(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            span: self.current_span().unwrap_or(Span {
                start: self.source.len(),
                end: self.source.len(),
                line: 0,
                col: 0,
            }),
        }
    }

    fn error_eof(&self, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            span: Span {
                start: self.source.len(),
                end: self.source.len(),
                line: self.current_span().map(|s| s.line).unwrap_or(0),
                col: self.current_span().map(|s| s.col).unwrap_or(0),
            },
        }
    }

    fn error(&self, token: Token, message: &str) -> ParseError {
        ParseError {
            message: message.to_string(),
            span: token.span,
        }
    }

    fn text_for_expr_name(&self, expr: &Expression) -> ParseResult<String> {
        self.expr_name(expr.clone())
    }

    fn expr_name(&self, expr: Expression) -> ParseResult<String> {
        match expr {
            Expression::Identifier(parts) => Ok(parts.join(".")),
            Expression::Paren(inner) => self.expr_name(*inner),
            _ => Err(self.error_current("expected function name")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionError {
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Clone, Default)]
pub struct BindParams {
    pub positional: Vec<SqlValue>,
    pub named: HashMap<String, SqlValue>,
}

pub fn bind_params(query: &Query, params: &BindParams) -> Result<Query, ExecutionError> {
    let mut positional = 0usize;
    bind_query(query, params, &mut positional)
}

fn bind_query(
    query: &Query,
    params: &BindParams,
    positional: &mut usize,
) -> Result<Query, ExecutionError> {
    Ok(Query {
        span: query.span,
        kind: match &query.kind {
            QueryKind::Select(select) => {
                QueryKind::Select(bind_select(select, params, positional)?)
            }
            QueryKind::Union { left, all, right } => QueryKind::Union {
                left: Box::new(bind_query(left, params, positional)?),
                all: *all,
                right: Box::new(bind_query(right, params, positional)?),
            },
        },
    })
}

fn bind_select(
    select: &SelectStatement,
    params: &BindParams,
    positional: &mut usize,
) -> Result<SelectStatement, ExecutionError> {
    Ok(SelectStatement {
        distinct: select.distinct,
        projection: select
            .projection
            .iter()
            .map(|item| bind_select_item(item, params, positional))
            .collect::<Result<_, _>>()?,
        from: select
            .from
            .iter()
            .map(|table| bind_table_ref(table, params, positional))
            .collect::<Result<_, _>>()?,
        where_clause: select
            .where_clause
            .as_ref()
            .map(|expr| bind_expression(expr, params, positional))
            .transpose()?,
        group_by: select
            .group_by
            .iter()
            .map(|expr| bind_expression(expr, params, positional))
            .collect::<Result<_, _>>()?,
        having: select
            .having
            .as_ref()
            .map(|expr| bind_expression(expr, params, positional))
            .transpose()?,
        order_by: select
            .order_by
            .iter()
            .map(|item| bind_order_by_item(item, params, positional))
            .collect::<Result<_, _>>()?,
        limit: select.limit,
    })
}

fn bind_select_item(
    item: &SelectItem,
    params: &BindParams,
    positional: &mut usize,
) -> Result<SelectItem, ExecutionError> {
    Ok(SelectItem {
        expr: bind_expression(&item.expr, params, positional)?,
        alias: item.alias.clone(),
    })
}

fn bind_table_ref(
    table: &TableRef,
    params: &BindParams,
    positional: &mut usize,
) -> Result<TableRef, ExecutionError> {
    Ok(TableRef {
        source: match &table.source {
            TableSource::Named(path) => TableSource::Named(path.clone()),
            TableSource::Subquery(query) => {
                TableSource::Subquery(Box::new(bind_query(query, params, positional)?))
            }
        },
        alias: table.alias.clone(),
        joins: table
            .joins
            .iter()
            .map(|join| bind_join_clause(join, params, positional))
            .collect::<Result<_, _>>()?,
    })
}

fn bind_join_clause(
    join: &JoinClause,
    params: &BindParams,
    positional: &mut usize,
) -> Result<JoinClause, ExecutionError> {
    Ok(JoinClause {
        kind: join.kind.clone(),
        table: bind_table_ref(&join.table, params, positional)?,
        on: join
            .on
            .as_ref()
            .map(|expr| bind_expression(expr, params, positional))
            .transpose()?,
    })
}

fn bind_order_by_item(
    item: &OrderByItem,
    params: &BindParams,
    positional: &mut usize,
) -> Result<OrderByItem, ExecutionError> {
    Ok(OrderByItem {
        expr: bind_expression(&item.expr, params, positional)?,
        descending: item.descending,
    })
}

fn bind_expression(
    expr: &Expression,
    params: &BindParams,
    positional: &mut usize,
) -> Result<Expression, ExecutionError> {
    Ok(match expr {
        Expression::BindParameter(Some(name)) => {
            let key = name.to_lowercase();
            let value = params.named.get(&key).ok_or_else(|| ExecutionError {
                message: format!("missing named QoQ parameter '{}'", name),
                span: None,
            })?;
            sql_value_to_expression(value)
        }
        Expression::BindParameter(None) => {
            let value = params
                .positional
                .get(*positional)
                .ok_or_else(|| ExecutionError {
                    message: "missing positional QoQ parameter".to_string(),
                    span: None,
                })?;
            *positional += 1;
            sql_value_to_expression(value)
        }
        Expression::FunctionCall { name, args } => Expression::FunctionCall {
            name: name.clone(),
            args: args
                .iter()
                .map(|arg| bind_expression(arg, params, positional))
                .collect::<Result<_, _>>()?,
        },
        Expression::Subquery(query) => {
            Expression::Subquery(Box::new(bind_query(query, params, positional)?))
        }
        Expression::Case {
            branches,
            else_expr,
        } => Expression::Case {
            branches: branches
                .iter()
                .map(|(cond, value)| {
                    Ok((
                        bind_expression(cond, params, positional)?,
                        bind_expression(value, params, positional)?,
                    ))
                })
                .collect::<Result<Vec<_>, ExecutionError>>()?,
            else_expr: else_expr
                .as_ref()
                .map(|expr| bind_expression(expr, params, positional).map(Box::new))
                .transpose()?,
        },
        Expression::Paren(inner) => {
            Expression::Paren(Box::new(bind_expression(inner, params, positional)?))
        }
        Expression::Unary { op, expr } => Expression::Unary {
            op: op.clone(),
            expr: Box::new(bind_expression(expr, params, positional)?),
        },
        Expression::Binary { left, op, right } => Expression::Binary {
            left: Box::new(bind_expression(left, params, positional)?),
            op: op.clone(),
            right: Box::new(bind_expression(right, params, positional)?),
        },
        Expression::IsNull { expr, negated } => Expression::IsNull {
            expr: Box::new(bind_expression(expr, params, positional)?),
            negated: *negated,
        },
        Expression::Identifier(parts) => Expression::Identifier(parts.clone()),
        Expression::String(s) => Expression::String(s.clone()),
        Expression::Number(n) => Expression::Number(n.clone()),
        Expression::Boolean(b) => Expression::Boolean(*b),
        Expression::Null => Expression::Null,
        Expression::OdbcDateTime(s) => Expression::OdbcDateTime(s.clone()),
        Expression::Star => Expression::Star,
        Expression::QualifiedStar(parts) => Expression::QualifiedStar(parts.clone()),
    })
}

fn sql_value_to_expression(value: &SqlValue) -> Expression {
    match value {
        SqlValue::Null => Expression::Null,
        SqlValue::Bool(b) => Expression::Boolean(*b),
        SqlValue::Int(i) => Expression::Number(i.to_string()),
        SqlValue::Float(f) => Expression::Number({
            let mut s = f.to_string();
            if s.ends_with(".0") {
                s.truncate(s.len() - 2);
            }
            s
        }),
        SqlValue::Text(s) => Expression::String(s.clone()),
        SqlValue::Bytes(bytes) => Expression::String(String::from_utf8_lossy(bytes).to_string()),
    }
}

pub fn execute(
    query: &Query,
    sources: &HashMap<String, QueryResult>,
) -> Result<QueryResult, ExecutionError> {
    execute_with_source_resolver(query, |path| {
        let key = path.join(".");
        Ok(sources
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(&key))
            .map(|(_, result)| {
                Box::new(QueryResultSource {
                    result: result.clone(),
                }) as Box<dyn QuerySource>
            }))
    })
}

pub fn execute_with_source_resolver<'a, F>(
    query: &Query,
    source_resolver: F,
) -> Result<QueryResult, ExecutionError>
where
    F: FnMut(&[String]) -> Result<Option<Box<dyn QuerySource + 'a>>, ExecutionError> + 'a,
{
    QueryExecutor::new(source_resolver).execute(query)
}

pub trait QuerySource {
    fn columns(&self) -> &[QueryColumn];
    fn row_count(&self) -> usize;
    fn value(&self, row_idx: usize, col_idx: usize) -> SqlValue;
}

#[derive(Clone, Debug)]
struct QueryResultSource {
    result: QueryResult,
}

impl QuerySource for QueryResultSource {
    fn columns(&self) -> &[QueryColumn] {
        &self.result.columns
    }

    fn row_count(&self) -> usize {
        self.result.rows.len()
    }

    fn value(&self, row_idx: usize, col_idx: usize) -> SqlValue {
        self.result
            .rows
            .get(row_idx)
            .and_then(|row| row.get(col_idx))
            .cloned()
            .unwrap_or(SqlValue::Null)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamingAggregate {
    Avg,
    Sum,
    Count,
    Min,
    Max,
}

#[derive(Clone, Debug)]
enum StreamingAggregateArg {
    Star,
    Identifier(Vec<String>),
}

#[derive(Clone, Debug)]
struct StreamingAggregatePlan {
    op: StreamingAggregate,
    arg: StreamingAggregateArg,
    output_name: String,
}

fn simple_streaming_aggregate_plan(select: &SelectStatement) -> Option<StreamingAggregatePlan> {
    let item = select.projection.first()?;
    let (name, args) = match &item.expr {
        Expression::FunctionCall { name, args } => (name, args),
        _ => return None,
    };
    let op = match name.last()?.to_lowercase().as_str() {
        "avg" => StreamingAggregate::Avg,
        "sum" => StreamingAggregate::Sum,
        "count" => StreamingAggregate::Count,
        "min" => StreamingAggregate::Min,
        "max" => StreamingAggregate::Max,
        _ => return None,
    };

    let arg = match args.as_slice() {
        [Expression::Star] if op == StreamingAggregate::Count => StreamingAggregateArg::Star,
        [Expression::Identifier(path)] => StreamingAggregateArg::Identifier(path.clone()),
        _ => return None,
    };

    Some(StreamingAggregatePlan {
        op,
        arg,
        output_name: item
            .alias
            .clone()
            .unwrap_or_else(|| projection_name(&item.expr, 0)),
    })
}

pub struct QueryExecutor<'a> {
    source_resolver: std::cell::RefCell<Box<SourceResolver<'a>>>,
}

type SourceResolver<'a> =
    dyn FnMut(&[String]) -> Result<Option<Box<dyn QuerySource + 'a>>, ExecutionError> + 'a;

impl<'a> QueryExecutor<'a> {
    pub fn new<F>(source_resolver: F) -> Self
    where
        F: FnMut(&[String]) -> Result<Option<Box<dyn QuerySource + 'a>>, ExecutionError> + 'a,
    {
        Self {
            source_resolver: std::cell::RefCell::new(Box::new(source_resolver)),
        }
    }

    pub fn execute(&self, query: &Query) -> Result<QueryResult, ExecutionError> {
        match &query.kind {
            QueryKind::Select(select) => {
                if let Some(result) = self.execute_fast_simple_aggregate(select)? {
                    return Ok(result);
                }
                self.execute_select(select)
            }
            QueryKind::Union { left, right, all } => {
                let mut left_result = self.execute(left)?;
                let right_result = self.execute(right)?;
                left_result.rows.extend(right_result.rows);
                left_result.columns =
                    merge_union_columns(&left_result.columns, &right_result.columns);
                if !all {
                    dedupe_rows(&mut left_result.rows);
                }
                Ok(left_result)
            }
        }
    }

    fn resolve_source(
        &self,
        path: &[String],
    ) -> Result<Option<Box<dyn QuerySource + 'a>>, ExecutionError> {
        (self.source_resolver.borrow_mut())(path)
    }

    fn execute_fast_simple_aggregate(
        &self,
        select: &SelectStatement,
    ) -> Result<Option<QueryResult>, ExecutionError> {
        if select.distinct
            || !select.group_by.is_empty()
            || select.having.is_some()
            || !select.order_by.is_empty()
            || select.limit.is_some()
            || select.where_clause.is_some()
            || select.from.len() != 1
            || !select.from[0].joins.is_empty()
            || !matches!(select.from[0].source, TableSource::Named(_))
            || select.projection.len() != 1
        {
            return Ok(None);
        }

        let Some(plan) = simple_streaming_aggregate_plan(select) else {
            return Ok(None);
        };

        let source_path = match &select.from[0].source {
            TableSource::Named(path) => path,
            TableSource::Subquery(_) => return Ok(None),
        };
        let source = self.resolve_source(source_path)?.ok_or_else(|| {
            self.error(format!("unknown table '{}'", source_path.join(".")), None)
        })?;

        let col_idx = match &plan.arg {
            StreamingAggregateArg::Star => None,
            StreamingAggregateArg::Identifier(path) => {
                let table_alias = select.from[0]
                    .alias
                    .as_deref()
                    .or_else(|| source_path.last().map(String::as_str));
                Some(resolve_source_column_index(
                    source.columns(),
                    source_path,
                    table_alias,
                    path,
                )?)
            }
        };

        let output = stream_simple_aggregate(source.as_ref(), plan.op, col_idx);

        Ok(Some(QueryResult {
            columns: vec![QueryColumn {
                name: plan.output_name,
                col_type: aggregate_output_type(plan.op, &output),
            }],
            rows: vec![vec![output]],
        }))
    }

    fn execute_select(&self, select: &SelectStatement) -> Result<QueryResult, ExecutionError> {
        let source_column_plan = SourceColumnDependencyPlan::for_select(select);
        let mut rows = self.resolve_from(&select.from, &source_column_plan)?;

        if let Some(expr) = &select.where_clause {
            rows.retain(|row| self.eval_truthy(expr, row, None).unwrap_or(false));
        }

        let grouped = select.group_by.iter().any(|_| true)
            || select
                .projection
                .iter()
                .any(|item| contains_aggregate(&item.expr))
            || select
                .having
                .as_ref()
                .map(|expr| contains_aggregate(expr))
                .unwrap_or(false);

        let mut output_rows = if grouped {
            self.execute_grouped(select, &rows)?
        } else {
            self.execute_rowwise(select, &rows)?
        };

        if select.distinct {
            dedupe_rows(&mut output_rows.rows);
        }

        if !select.order_by.is_empty() {
            self.apply_order_by(select, &mut output_rows)?;
        }

        if let Some(limit) = select.limit {
            output_rows.rows.truncate(limit as usize);
        }

        Ok(output_rows)
    }

    fn execute_rowwise(
        &self,
        select: &SelectStatement,
        rows: &[ResolvedRow],
    ) -> Result<QueryResult, ExecutionError> {
        if let Some(result) = self.execute_simple_identifier_projection(select, rows) {
            return Ok(result);
        }

        let mut out_rows = Vec::new();
        let mut columns = Vec::new();
        for row in rows {
            let mut out_row = Vec::new();
            for item in &select.projection {
                let value = match &item.expr {
                    Expression::Star => {
                        return Err(self.error("star projection requires query expansion", None));
                    }
                    Expression::QualifiedStar(_) => {
                        return Err(
                            self.error("qualified star projection requires query expansion", None)
                        );
                    }
                    expr => self.eval(expr, row, None)?,
                };
                out_row.push(value);
            }
            if columns.is_empty() {
                columns = self.projection_columns(select, row)?;
            }
            out_rows.push(out_row);
        }
        Ok(QueryResult {
            columns,
            rows: out_rows,
        })
    }

    fn execute_simple_identifier_projection(
        &self,
        select: &SelectStatement,
        rows: &[ResolvedRow],
    ) -> Option<QueryResult> {
        let first = rows.first()?;
        let mut indices = Vec::with_capacity(select.projection.len());
        for item in &select.projection {
            match &item.expr {
                Expression::Identifier(path) => indices.push(first.lookup_path_index(path)?),
                _ => return None,
            }
        }

        let columns = select
            .projection
            .iter()
            .zip(indices.iter())
            .enumerate()
            .map(|(idx, (item, value_idx))| {
                let value = first.values.get(*value_idx).unwrap_or(&SqlValue::Null);
                QueryColumn {
                    name: item
                        .alias
                        .clone()
                        .unwrap_or_else(|| projection_name(&item.expr, idx)),
                    col_type: infer_query_type(value),
                }
            })
            .collect();

        let out_rows = rows
            .iter()
            .map(|row| {
                indices
                    .iter()
                    .map(|idx| row.values.get(*idx).cloned().unwrap_or(SqlValue::Null))
                    .collect()
            })
            .collect();

        Some(QueryResult {
            columns,
            rows: out_rows,
        })
    }

    fn execute_grouped(
        &self,
        select: &SelectStatement,
        rows: &[ResolvedRow],
    ) -> Result<QueryResult, ExecutionError> {
        let mut groups: HashMap<Vec<String>, Vec<&ResolvedRow>> = HashMap::new();
        if select.group_by.is_empty() {
            groups.insert(vec!["__all__".to_string()], rows.iter().collect());
        } else {
            for row in rows {
                let mut key = Vec::new();
                for expr in &select.group_by {
                    key.push(sql_value_key(&self.eval(expr, row, None)?));
                }
                groups.entry(key).or_default().push(row);
            }
        }

        let mut out_rows = Vec::new();
        let mut columns = Vec::new();
        for group_rows in groups.values() {
            let first = *group_rows
                .first()
                .ok_or_else(|| self.error("empty group", None))?;
            if let Some(having) = &select.having {
                if !self.eval_truthy(having, first, Some(group_rows))? {
                    continue;
                }
            }

            let mut out_row = Vec::new();
            for item in &select.projection {
                let value = self.eval(&item.expr, first, Some(group_rows))?;
                out_row.push(value);
            }
            if columns.is_empty() {
                columns = self.projection_columns(select, first)?;
            }
            out_rows.push(out_row);
        }
        Ok(QueryResult {
            columns,
            rows: out_rows,
        })
    }

    fn projection_columns(
        &self,
        select: &SelectStatement,
        row: &ResolvedRow,
    ) -> Result<Vec<QueryColumn>, ExecutionError> {
        let mut columns = Vec::new();
        for (idx, item) in select.projection.iter().enumerate() {
            let name = item
                .alias
                .clone()
                .unwrap_or_else(|| projection_name(&item.expr, idx));
            let value = match &item.expr {
                Expression::Star | Expression::QualifiedStar(_) => SqlValue::Null,
                expr => self.eval(expr, row, None).unwrap_or(SqlValue::Null),
            };
            columns.push(QueryColumn {
                name,
                col_type: infer_query_type(&value),
            });
        }
        Ok(columns)
    }

    fn apply_order_by(
        &self,
        select: &SelectStatement,
        result: &mut QueryResult,
    ) -> Result<(), ExecutionError> {
        let order_specs = resolve_result_order_specs(select, &result.columns);
        result.rows.sort_by(|a, b| {
            for (idx, descending) in &order_specs {
                let ord = a
                    .get(*idx)
                    .zip(b.get(*idx))
                    .and_then(|(l, r)| compare_sql_values(l, r))
                    .unwrap_or(Ordering::Equal);
                if ord != Ordering::Equal {
                    return if *descending { ord.reverse() } else { ord };
                }
            }
            Ordering::Equal
        });
        Ok(())
    }

    fn resolve_from(
        &self,
        from: &[TableRef],
        source_column_plan: &SourceColumnDependencyPlan,
    ) -> Result<Vec<ResolvedRow>, ExecutionError> {
        if from.is_empty() {
            return Ok(vec![ResolvedRow::default()]);
        }

        let mut current = self.resolve_table_ref(&from[0], source_column_plan)?;
        for table in &from[1..] {
            let next = self.resolve_table_ref(table, source_column_plan)?;
            current = cross_join(&current, &next, None)?;
        }
        Ok(current.rows)
    }

    fn resolve_table_ref(
        &self,
        table: &TableRef,
        source_column_plan: &SourceColumnDependencyPlan,
    ) -> Result<ResolvedTable, ExecutionError> {
        let base = match &table.source {
            TableSource::Named(path) => self
                .resolve_source(path)?
                .ok_or_else(|| self.error(format!("unknown table '{}'", path.join(".")), None))?,
            TableSource::Subquery(query) => {
                let result = self.execute(query)?;
                Box::new(QueryResultSource { result }) as Box<dyn QuerySource>
            }
        };

        let alias = table
            .alias
            .clone()
            .or_else(|| match &table.source {
                TableSource::Named(path) => path.last().cloned(),
                TableSource::Subquery(_) => Some("subquery".to_string()),
            })
            .unwrap_or_else(|| "query".to_string());

        let source_columns = match &table.source {
            TableSource::Named(path) if source_column_plan.safe_for_pruning => {
                source_column_plan.source_for(path, &alias)
            }
            _ => None,
        };

        let mut resolved = materialize_table(base.as_ref(), &alias, source_columns);
        for join in &table.joins {
            let right = self.resolve_table_ref(&join.table, source_column_plan)?;
            resolved = apply_join(resolved, right, join, self)?;
        }
        Ok(resolved)
    }

    fn eval_truthy(
        &self,
        expr: &Expression,
        row: &ResolvedRow,
        group_rows: Option<&[&ResolvedRow]>,
    ) -> Result<bool, ExecutionError> {
        Ok(is_truthy(&self.eval(expr, row, group_rows)?))
    }

    fn eval(
        &self,
        expr: &Expression,
        row: &ResolvedRow,
        group_rows: Option<&[&ResolvedRow]>,
    ) -> Result<SqlValue, ExecutionError> {
        match expr {
            Expression::Identifier(path) => row.lookup_path(path).cloned().ok_or_else(|| {
                self.error(
                    format!("unknown identifier '{}'", identifier_path(path)),
                    None,
                )
            }),
            Expression::String(s) => Ok(SqlValue::Text(s.clone())),
            Expression::Number(n) => parse_sql_number(n),
            Expression::Boolean(b) => Ok(SqlValue::Bool(*b)),
            Expression::Null => Ok(SqlValue::Null),
            Expression::OdbcDateTime(s) => Ok(SqlValue::Text(s.clone())),
            Expression::BindParameter(_) => Err(self.error(
                "bind parameters are not supported in QoQ execution yet",
                None,
            )),
            Expression::Star | Expression::QualifiedStar(_) => {
                Err(self.error("star is only valid in select projection", None))
            }
            Expression::Paren(inner) => self.eval(inner, row, group_rows),
            Expression::Unary { op, expr } => {
                let value = self.eval(expr, row, group_rows)?;
                match op {
                    UnaryOp::Not => Ok(SqlValue::Bool(!is_truthy(&value))),
                    UnaryOp::Negate => negate_sql_value(value),
                }
            }
            Expression::Binary { left, op, right } => {
                let left = self.eval(left, row, group_rows)?;
                let right = self.eval(right, row, group_rows)?;
                eval_binary(left, op, right)
            }
            Expression::IsNull { expr, negated } => {
                let value = self.eval(expr, row, group_rows)?;
                Ok(SqlValue::Bool(if *negated {
                    !matches!(value, SqlValue::Null)
                } else {
                    matches!(value, SqlValue::Null)
                }))
            }
            Expression::FunctionCall { name, args } => {
                self.eval_function(name, args, row, group_rows)
            }
            Expression::Subquery(query) => {
                let result = self.execute(query)?;
                if result.rows.len() == 1 && result.rows[0].len() == 1 {
                    Ok(result.rows[0][0].clone())
                } else {
                    Err(self.error(
                        "subquery expressions must return a single value",
                        Some(query.span),
                    ))
                }
            }
            Expression::Case {
                branches,
                else_expr,
            } => {
                for (cond, value) in branches {
                    if is_truthy(&self.eval(cond, row, group_rows)?) {
                        return self.eval(value, row, group_rows);
                    }
                }
                if let Some(expr) = else_expr {
                    self.eval(expr, row, group_rows)
                } else {
                    Ok(SqlValue::Null)
                }
            }
        }
    }

    fn eval_function(
        &self,
        name: &[String],
        args: &[Expression],
        row: &ResolvedRow,
        group_rows: Option<&[&ResolvedRow]>,
    ) -> Result<SqlValue, ExecutionError> {
        let fname = name.last().map(|s| s.to_lowercase()).unwrap_or_default();
        if let Some(group_rows) = group_rows {
            match fname.as_str() {
                "count" => {
                    if args.is_empty() || matches!(args.first(), Some(Expression::Star)) {
                        return Ok(SqlValue::Int(group_rows.len() as i64));
                    }
                    if let Some(count) = aggregate_count_identifier(group_rows, &args[0]) {
                        return Ok(SqlValue::Int(count));
                    }
                    let mut count = 0i64;
                    for r in group_rows {
                        if !matches!(self.eval(&args[0], r, None)?, SqlValue::Null) {
                            count += 1;
                        }
                    }
                    return Ok(SqlValue::Int(count));
                }
                "sum" => {
                    return aggregate_numeric(
                        group_rows,
                        |acc, v| acc + v,
                        0.0,
                        &args[0],
                        self,
                        row,
                    );
                }
                "min" => return aggregate_minmax(group_rows, true, &args[0], self, row),
                "max" => return aggregate_minmax(group_rows, false, &args[0], self, row),
                "avg" => {
                    if let Some(value) = aggregate_avg_identifier(group_rows, &args[0]) {
                        return Ok(value);
                    }
                    let mut total = 0.0;
                    let mut count = 0.0;
                    for r in group_rows {
                        let val = self.eval(&args[0], r, None)?;
                        if let Some(n) = sql_as_f64(&val) {
                            total += n;
                            count += 1.0;
                        }
                    }
                    return Ok(if count == 0.0 {
                        SqlValue::Null
                    } else {
                        SqlValue::Float(total / count)
                    });
                }
                _ => {}
            }
        }

        match fname.as_str() {
            "lower" => {
                let value = self.eval(&args[0], row, None)?;
                Ok(SqlValue::Text(sql_to_string(&value).to_lowercase()))
            }
            "upper" => {
                let value = self.eval(&args[0], row, None)?;
                Ok(SqlValue::Text(sql_to_string(&value).to_uppercase()))
            }
            "trim" => {
                let value = self.eval(&args[0], row, None)?;
                Ok(SqlValue::Text(sql_to_string(&value).trim().to_string()))
            }
            "count" => Ok(SqlValue::Int(1)),
            _ => Err(self.error(format!("unsupported QoQ function '{}'", fname), None)),
        }
    }

    fn error(&self, message: impl Into<String>, span: Option<Span>) -> ExecutionError {
        ExecutionError {
            message: message.into(),
            span,
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedRow {
    values: Vec<SqlValue>,
    lookup: Arc<HashMap<String, usize>>,
}

impl ResolvedRow {
    fn lookup_path(&self, path: &[String]) -> Option<&SqlValue> {
        self.lookup_path_index(path)
            .and_then(|idx| self.values.get(idx))
    }

    fn lookup_path_index(&self, path: &[String]) -> Option<usize> {
        match path {
            [single] => self.lookup.get(&single.to_lowercase()).copied(),
            _ => self
                .lookup
                .get(&identifier_path(path).to_lowercase())
                .copied(),
        }
    }
}

impl Default for ResolvedRow {
    fn default() -> Self {
        Self {
            values: Vec::new(),
            lookup: Arc::new(HashMap::new()),
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedTable {
    columns: Vec<QueryColumn>,
    lookup: Arc<HashMap<String, usize>>,
    rows: Vec<ResolvedRow>,
}

fn materialize_table(
    source: &dyn QuerySource,
    alias: &str,
    source_columns: Option<&SourceColumnDependency>,
) -> ResolvedTable {
    let source_column_indices =
        materialized_source_column_indices(source.columns(), source_columns);
    let columns = source_column_indices
        .iter()
        .map(|idx| source.columns()[*idx].clone())
        .collect::<Vec<_>>();
    let lookup = Arc::new(column_lookup(&columns, alias, 0));
    let rows = (0..source.row_count())
        .map(|row_idx| ResolvedRow {
            values: source_column_indices
                .iter()
                .map(|col_idx| source.value(row_idx, *col_idx))
                .collect(),
            lookup: lookup.clone(),
        })
        .collect();

    ResolvedTable {
        columns,
        lookup,
        rows,
    }
}

fn materialized_source_column_indices(
    columns: &[QueryColumn],
    source_columns: Option<&SourceColumnDependency>,
) -> Vec<usize> {
    let Some(source_columns) = source_columns else {
        return (0..columns.len()).collect();
    };
    if source_columns.all_columns_required {
        return (0..columns.len()).collect();
    }

    let required: HashSet<_> = source_columns
        .columns
        .iter()
        .map(|column| column.name.to_lowercase())
        .collect();

    columns
        .iter()
        .enumerate()
        .filter_map(|(idx, column)| {
            required
                .contains(&column.name.to_lowercase())
                .then_some(idx)
        })
        .collect()
}

fn cross_join(
    left: &ResolvedTable,
    right: &ResolvedTable,
    on: Option<&Expression>,
) -> Result<ResolvedTable, ExecutionError> {
    let mut rows = Vec::new();
    let columns = merge_columns(&left.columns, &right.columns);
    let lookup = merge_lookups(left, right);
    for l in &left.rows {
        for r in &right.rows {
            let merged = merge_rows(l, r, lookup.clone());
            if let Some(expr) = on {
                // `on` is evaluated by the caller once a QueryExecutor exists.
                let _ = expr;
            }
            rows.push(merged);
        }
    }

    Ok(ResolvedTable {
        columns,
        lookup,
        rows,
    })
}

fn apply_join(
    left: ResolvedTable,
    right: ResolvedTable,
    join: &JoinClause,
    executor: &QueryExecutor<'_>,
) -> Result<ResolvedTable, ExecutionError> {
    let mut rows = Vec::new();
    let mut matched_right = vec![false; right.rows.len()];
    let columns = merge_columns(&left.columns, &right.columns);
    let lookup = merge_lookups(&left, &right);

    for l in &left.rows {
        let mut matched = false;
        for (idx, r) in right.rows.iter().enumerate() {
            let merged = merge_rows(l, r, lookup.clone());
            let passes = match &join.on {
                Some(expr) => executor.eval_truthy(expr, &merged, None)?,
                None => true,
            };
            if passes {
                matched = true;
                matched_right[idx] = true;
                rows.push(merged);
            }
        }
        if !matched && matches!(join.kind, JoinKind::Left | JoinKind::Full) {
            rows.push(pad_right_row(l, right.columns.len(), lookup.clone()));
        }
    }

    if matches!(join.kind, JoinKind::Right | JoinKind::Full) {
        for (idx, r) in right.rows.iter().enumerate() {
            if !matched_right[idx] {
                rows.push(pad_left_row(r, left.columns.len(), lookup.clone()));
            }
        }
    }

    Ok(ResolvedTable {
        columns,
        lookup,
        rows,
    })
}

fn column_lookup(columns: &[QueryColumn], alias: &str, offset: usize) -> HashMap<String, usize> {
    let mut lookup = HashMap::with_capacity(columns.len() * 2);
    for (idx, col) in columns.iter().enumerate() {
        let absolute_idx = offset + idx;
        lookup.insert(col.name.to_lowercase(), absolute_idx);
        lookup.insert(format!("{alias}.{}", col.name).to_lowercase(), absolute_idx);
    }
    lookup
}

fn merge_lookups(left: &ResolvedTable, right: &ResolvedTable) -> Arc<HashMap<String, usize>> {
    let mut lookup = HashMap::with_capacity(left.lookup.len() + right.lookup.len());
    for (key, idx) in left.lookup.iter() {
        lookup.insert(key.clone(), *idx);
    }
    let offset = left.columns.len();
    for (key, idx) in right.lookup.iter() {
        lookup.insert(key.clone(), offset + *idx);
    }
    Arc::new(lookup)
}

fn merge_rows(
    left: &ResolvedRow,
    right: &ResolvedRow,
    lookup: Arc<HashMap<String, usize>>,
) -> ResolvedRow {
    let mut values = Vec::with_capacity(left.values.len() + right.values.len());
    values.extend(left.values.iter().cloned());
    values.extend(right.values.iter().cloned());
    ResolvedRow { values, lookup }
}

fn pad_right_row(
    left: &ResolvedRow,
    right_len: usize,
    lookup: Arc<HashMap<String, usize>>,
) -> ResolvedRow {
    let mut values = Vec::with_capacity(left.values.len() + right_len);
    values.extend(left.values.iter().cloned());
    values.extend((0..right_len).map(|_| SqlValue::Null));
    ResolvedRow { values, lookup }
}

fn pad_left_row(
    right: &ResolvedRow,
    left_len: usize,
    lookup: Arc<HashMap<String, usize>>,
) -> ResolvedRow {
    let mut values = Vec::with_capacity(left_len + right.values.len());
    values.extend((0..left_len).map(|_| SqlValue::Null));
    values.extend(right.values.iter().cloned());
    ResolvedRow { values, lookup }
}

fn merge_columns(left: &[QueryColumn], right: &[QueryColumn]) -> Vec<QueryColumn> {
    let mut columns = left.to_vec();
    columns.extend(right.iter().cloned());
    columns
}

fn resolve_source_column_index(
    columns: &[QueryColumn],
    source_path: &[String],
    table_alias: Option<&str>,
    identifier: &[String],
) -> Result<usize, ExecutionError> {
    let column_name = match identifier {
        [single] => single.as_str(),
        [qualifier, column]
            if table_alias.is_some_and(|alias| alias.eq_ignore_ascii_case(qualifier))
                || source_path
                    .last()
                    .is_some_and(|name| name.eq_ignore_ascii_case(qualifier)) =>
        {
            column.as_str()
        }
        _ => {
            return Err(ExecutionError {
                message: format!("unknown identifier '{}'", identifier_path(identifier)),
                span: None,
            });
        }
    };

    columns
        .iter()
        .position(|c| c.name.eq_ignore_ascii_case(column_name))
        .ok_or_else(|| ExecutionError {
            message: format!("unknown identifier '{}'", identifier_path(identifier)),
            span: None,
        })
}

fn stream_simple_aggregate(
    source: &dyn QuerySource,
    op: StreamingAggregate,
    col_idx: Option<usize>,
) -> SqlValue {
    match op {
        StreamingAggregate::Count => {
            if let Some(col_idx) = col_idx {
                let count = (0..source.row_count())
                    .filter(|row_idx| !matches!(source.value(*row_idx, col_idx), SqlValue::Null))
                    .count() as i64;
                SqlValue::Int(count)
            } else {
                SqlValue::Int(source.row_count() as i64)
            }
        }
        StreamingAggregate::Avg => {
            let Some(col_idx) = col_idx else {
                return SqlValue::Null;
            };
            let mut total = 0.0;
            let mut count = 0.0;
            for row_idx in 0..source.row_count() {
                if let Some(value) = sql_as_f64(&source.value(row_idx, col_idx)) {
                    total += value;
                    count += 1.0;
                }
            }
            if count == 0.0 {
                SqlValue::Null
            } else {
                SqlValue::Float(total / count)
            }
        }
        StreamingAggregate::Sum => {
            let Some(col_idx) = col_idx else {
                return SqlValue::Null;
            };
            let mut total = 0.0;
            let mut seen = false;
            for row_idx in 0..source.row_count() {
                if let Some(value) = sql_as_f64(&source.value(row_idx, col_idx)) {
                    total += value;
                    seen = true;
                }
            }
            if seen {
                SqlValue::Float(total)
            } else {
                SqlValue::Null
            }
        }
        StreamingAggregate::Min | StreamingAggregate::Max => {
            let Some(col_idx) = col_idx else {
                return SqlValue::Null;
            };
            let mut best: Option<SqlValue> = None;
            for row_idx in 0..source.row_count() {
                let value = source.value(row_idx, col_idx);
                best = match best {
                    None => Some(value),
                    Some(prev) => {
                        if let Some(ord) = compare_sql_values(&value, &prev) {
                            let better = (op == StreamingAggregate::Min && ord == Ordering::Less)
                                || (op == StreamingAggregate::Max && ord == Ordering::Greater);
                            if better { Some(value) } else { Some(prev) }
                        } else {
                            Some(prev)
                        }
                    }
                };
            }
            best.unwrap_or(SqlValue::Null)
        }
    }
}

fn aggregate_output_type(op: StreamingAggregate, value: &SqlValue) -> QueryColumnType {
    match op {
        StreamingAggregate::Avg | StreamingAggregate::Sum => QueryColumnType::Double,
        StreamingAggregate::Count => QueryColumnType::Integer,
        StreamingAggregate::Min | StreamingAggregate::Max => infer_query_type(value),
    }
}

fn resolve_result_order_specs(
    select: &SelectStatement,
    columns: &[QueryColumn],
) -> Vec<(usize, bool)> {
    select
        .order_by
        .iter()
        .filter_map(|spec| {
            let idx = match &spec.expr {
                Expression::Number(n) => n.parse::<usize>().ok().and_then(|n| n.checked_sub(1)),
                Expression::Identifier(path) if path.len() == 1 => columns
                    .iter()
                    .position(|c| c.name.eq_ignore_ascii_case(&path[0])),
                _ => None,
            }?;
            Some((idx, spec.descending))
        })
        .collect()
}

fn identifier_path(path: &[String]) -> String {
    path.join(".")
}

fn merge_union_columns(left: &[QueryColumn], right: &[QueryColumn]) -> Vec<QueryColumn> {
    if left.len() >= right.len() {
        left.to_vec()
    } else {
        right.to_vec()
    }
}

fn dedupe_rows(rows: &mut Vec<Vec<SqlValue>>) {
    let mut seen = HashSet::new();
    rows.retain(|row| seen.insert(row_key(row)));
}

fn row_key(row: &[SqlValue]) -> String {
    row.iter().map(sql_value_key).collect::<Vec<_>>().join("|")
}

fn sql_value_key(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => "null".to_string(),
        SqlValue::Bool(b) => format!("b:{b}"),
        SqlValue::Int(i) => format!("i:{i}"),
        SqlValue::Float(f) => format!("f:{f}"),
        SqlValue::Text(s) => format!("s:{s}"),
        SqlValue::Bytes(b) => format!("x:{:x?}", b),
    }
}

fn compare_sql_values(left: &SqlValue, right: &SqlValue) -> Option<Ordering> {
    match (left, right) {
        (SqlValue::Null, SqlValue::Null) => Some(Ordering::Equal),
        (SqlValue::Bool(a), SqlValue::Bool(b)) => Some(a.cmp(b)),
        _ => {
            if let (Some(a), Some(b)) = (sql_as_f64(left), sql_as_f64(right)) {
                a.partial_cmp(&b)
            } else {
                Some(sql_to_string(left).cmp(&sql_to_string(right)))
            }
        }
    }
}

fn eval_binary(left: SqlValue, op: &BinaryOp, right: SqlValue) -> Result<SqlValue, ExecutionError> {
    Ok(match op {
        BinaryOp::Or => SqlValue::Bool(is_truthy(&left) || is_truthy(&right)),
        BinaryOp::And => SqlValue::Bool(is_truthy(&left) && is_truthy(&right)),
        BinaryOp::Eq => SqlValue::Bool(sql_equals(&left, &right)),
        BinaryOp::NotEq => SqlValue::Bool(!sql_equals(&left, &right)),
        BinaryOp::Lt => {
            SqlValue::Bool(compare_sql_values(&left, &right).is_some_and(|o| o == Ordering::Less))
        }
        BinaryOp::Lte => SqlValue::Bool(
            compare_sql_values(&left, &right).is_some_and(|o| o != Ordering::Greater),
        ),
        BinaryOp::Gt => SqlValue::Bool(
            compare_sql_values(&left, &right).is_some_and(|o| o == Ordering::Greater),
        ),
        BinaryOp::Gte => {
            SqlValue::Bool(compare_sql_values(&left, &right).is_some_and(|o| o != Ordering::Less))
        }
        BinaryOp::Like => SqlValue::Bool(sql_to_string(&left).contains(&sql_to_string(&right))),
        BinaryOp::Contains => SqlValue::Bool(sql_to_string(&left).contains(&sql_to_string(&right))),
        BinaryOp::InstanceOf => {
            SqlValue::Bool(sql_to_string(&left).eq_ignore_ascii_case(&sql_to_string(&right)))
        }
        BinaryOp::CastAs => left,
        BinaryOp::Xor => SqlValue::Bool(is_truthy(&left) ^ is_truthy(&right)),
        BinaryOp::Eqv => SqlValue::Bool(is_truthy(&left) == is_truthy(&right)),
        BinaryOp::Add => numeric_binop(left, right, |a, b| a + b)?,
        BinaryOp::Sub => numeric_binop(left, right, |a, b| a - b)?,
        BinaryOp::Mul => numeric_binop(left, right, |a, b| a * b)?,
        BinaryOp::Div => numeric_binop(left, right, |a, b| a / b)?,
        BinaryOp::Mod => numeric_binop(left, right, |a, b| a % b)?,
        BinaryOp::Concat => {
            SqlValue::Text(format!("{}{}", sql_to_string(&left), sql_to_string(&right)))
        }
        BinaryOp::Pow => numeric_binop(left, right, |a, b| a.powf(b))?,
    })
}

fn numeric_binop(
    left: SqlValue,
    right: SqlValue,
    op: impl FnOnce(f64, f64) -> f64,
) -> Result<SqlValue, ExecutionError> {
    let a = sql_as_f64(&left).ok_or_else(|| ExecutionError {
        message: "numeric operator expected number".to_string(),
        span: None,
    })?;
    let b = sql_as_f64(&right).ok_or_else(|| ExecutionError {
        message: "numeric operator expected number".to_string(),
        span: None,
    })?;
    Ok(SqlValue::Float(op(a, b)))
}

fn negate_sql_value(value: SqlValue) -> Result<SqlValue, ExecutionError> {
    Ok(SqlValue::Float(-sql_as_f64(&value).ok_or_else(|| {
        ExecutionError {
            message: "unary - expected number".to_string(),
            span: None,
        }
    })?))
}

fn parse_sql_number(text: &str) -> Result<SqlValue, ExecutionError> {
    if let Ok(i) = text.parse::<i64>() {
        Ok(SqlValue::Int(i))
    } else if let Ok(f) = text.parse::<f64>() {
        Ok(SqlValue::Float(f))
    } else {
        Err(ExecutionError {
            message: format!("invalid number literal '{text}'"),
            span: None,
        })
    }
}

fn sql_to_string(value: &SqlValue) -> String {
    match value {
        SqlValue::Null => String::new(),
        SqlValue::Bool(b) => b.to_string(),
        SqlValue::Int(i) => i.to_string(),
        SqlValue::Float(f) => {
            let mut s = f.to_string();
            if s.ends_with(".0") {
                s.truncate(s.len() - 2);
            }
            s
        }
        SqlValue::Text(s) => s.clone(),
        SqlValue::Bytes(b) => String::from_utf8_lossy(b).to_string(),
    }
}

fn sql_as_f64(value: &SqlValue) -> Option<f64> {
    match value {
        SqlValue::Int(i) => Some(*i as f64),
        SqlValue::Float(f) => Some(*f),
        SqlValue::Text(s) => s.parse::<f64>().ok(),
        _ => None,
    }
}

fn sql_as_f64_ref(value: &SqlValue) -> Option<f64> {
    sql_as_f64(value)
}

fn is_truthy(value: &SqlValue) -> bool {
    match value {
        SqlValue::Null => false,
        SqlValue::Bool(b) => *b,
        SqlValue::Int(i) => *i != 0,
        SqlValue::Float(f) => *f != 0.0,
        SqlValue::Text(s) => !s.is_empty(),
        SqlValue::Bytes(b) => !b.is_empty(),
    }
}

fn sql_equals(left: &SqlValue, right: &SqlValue) -> bool {
    match (left, right) {
        (SqlValue::Null, SqlValue::Null) => true,
        (SqlValue::Bool(a), SqlValue::Bool(b)) => a == b,
        _ => {
            if let (Some(a), Some(b)) = (sql_as_f64(left), sql_as_f64(right)) {
                (a - b).abs() < f64::EPSILON
            } else {
                sql_to_string(left) == sql_to_string(right)
            }
        }
    }
}

fn infer_query_type(value: &SqlValue) -> QueryColumnType {
    match value {
        SqlValue::Null => QueryColumnType::Null,
        SqlValue::Bool(_) => QueryColumnType::Boolean,
        SqlValue::Int(_) => QueryColumnType::Integer,
        SqlValue::Float(_) => QueryColumnType::Double,
        SqlValue::Text(_) => QueryColumnType::Varchar,
        SqlValue::Bytes(_) => QueryColumnType::Blob,
    }
}

fn projection_name(expr: &Expression, idx: usize) -> String {
    match expr {
        Expression::Identifier(parts) => parts
            .last()
            .cloned()
            .unwrap_or_else(|| format!("column{}", idx + 1)),
        Expression::FunctionCall { name, .. } => name
            .last()
            .cloned()
            .unwrap_or_else(|| format!("expr{}", idx + 1)),
        _ => format!("expr{}", idx + 1),
    }
}

fn contains_aggregate(expr: &Expression) -> bool {
    match expr {
        Expression::FunctionCall { name, .. } => is_aggregate_name(name),
        Expression::Binary { left, right, .. } => {
            contains_aggregate(left) || contains_aggregate(right)
        }
        Expression::Unary { expr, .. }
        | Expression::Paren(expr)
        | Expression::IsNull { expr, .. } => contains_aggregate(expr),
        Expression::Case {
            branches,
            else_expr,
        } => {
            branches
                .iter()
                .any(|(c, v)| contains_aggregate(c) || contains_aggregate(v))
                || else_expr.as_deref().is_some_and(contains_aggregate)
        }
        _ => false,
    }
}

fn is_aggregate_name(name: &[String]) -> bool {
    matches!(
        name.last().map(|s| s.to_lowercase()).as_deref(),
        Some("count" | "sum" | "min" | "max" | "avg")
    )
}

fn is_count_name(name: &[String]) -> bool {
    name.last()
        .is_some_and(|name| name.eq_ignore_ascii_case("count"))
}

fn aggregate_numeric(
    group_rows: &[&ResolvedRow],
    op: impl Fn(f64, f64) -> f64 + Copy,
    init: f64,
    expr: &Expression,
    executor: &QueryExecutor<'_>,
    current: &ResolvedRow,
) -> Result<SqlValue, ExecutionError> {
    if let Some(value) = aggregate_numeric_identifier(group_rows, op, init, expr) {
        return Ok(value);
    }

    let mut acc = init;
    let mut first = true;
    for row in group_rows {
        if let Some(v) = sql_as_f64(&executor.eval(expr, row, None)?) {
            acc = if first { v } else { op(acc, v) };
            first = false;
        }
    }
    if first {
        Ok(SqlValue::Null)
    } else {
        let _ = current;
        Ok(SqlValue::Float(acc))
    }
}

fn aggregate_minmax(
    group_rows: &[&ResolvedRow],
    min: bool,
    expr: &Expression,
    executor: &QueryExecutor<'_>,
    current: &ResolvedRow,
) -> Result<SqlValue, ExecutionError> {
    if let Some(value) = aggregate_minmax_identifier(group_rows, min, expr) {
        return Ok(value);
    }

    let mut best: Option<SqlValue> = None;
    for row in group_rows {
        let value = executor.eval(expr, row, None)?;
        best = match best {
            None => Some(value),
            Some(prev) => {
                if let Some(ord) = compare_sql_values(&value, &prev) {
                    if (min && ord == Ordering::Less) || (!min && ord == Ordering::Greater) {
                        Some(value)
                    } else {
                        Some(prev)
                    }
                } else {
                    Some(prev)
                }
            }
        };
    }
    let _ = current;
    Ok(best.unwrap_or(SqlValue::Null))
}

fn aggregate_count_identifier(group_rows: &[&ResolvedRow], expr: &Expression) -> Option<i64> {
    let idx = aggregate_identifier_index(group_rows, expr)?;
    Some(
        group_rows
            .iter()
            .filter(|row| !matches!(row.values.get(idx), Some(SqlValue::Null) | None))
            .count() as i64,
    )
}

fn aggregate_avg_identifier(group_rows: &[&ResolvedRow], expr: &Expression) -> Option<SqlValue> {
    let idx = aggregate_identifier_index(group_rows, expr)?;
    let mut total = 0.0;
    let mut count = 0.0;
    for row in group_rows {
        if let Some(value) = row.values.get(idx).and_then(sql_as_f64_ref) {
            total += value;
            count += 1.0;
        }
    }
    Some(if count == 0.0 {
        SqlValue::Null
    } else {
        SqlValue::Float(total / count)
    })
}

fn aggregate_numeric_identifier(
    group_rows: &[&ResolvedRow],
    op: impl Fn(f64, f64) -> f64 + Copy,
    init: f64,
    expr: &Expression,
) -> Option<SqlValue> {
    let idx = aggregate_identifier_index(group_rows, expr)?;
    let mut acc = init;
    let mut first = true;
    for row in group_rows {
        if let Some(value) = row.values.get(idx).and_then(sql_as_f64_ref) {
            acc = if first { value } else { op(acc, value) };
            first = false;
        }
    }
    Some(if first {
        SqlValue::Null
    } else {
        SqlValue::Float(acc)
    })
}

fn aggregate_minmax_identifier(
    group_rows: &[&ResolvedRow],
    min: bool,
    expr: &Expression,
) -> Option<SqlValue> {
    let idx = aggregate_identifier_index(group_rows, expr)?;
    let mut best: Option<SqlValue> = None;
    for row in group_rows {
        let value = row.values.get(idx).cloned().unwrap_or(SqlValue::Null);
        best = match best {
            None => Some(value),
            Some(prev) => {
                if let Some(ord) = compare_sql_values(&value, &prev) {
                    if (min && ord == Ordering::Less) || (!min && ord == Ordering::Greater) {
                        Some(value)
                    } else {
                        Some(prev)
                    }
                } else {
                    Some(prev)
                }
            }
        };
    }
    Some(best.unwrap_or(SqlValue::Null))
}

fn aggregate_identifier_index(group_rows: &[&ResolvedRow], expr: &Expression) -> Option<usize> {
    match expr {
        Expression::Identifier(path) => group_rows.first()?.lookup_path_index(path),
        _ => None,
    }
}
