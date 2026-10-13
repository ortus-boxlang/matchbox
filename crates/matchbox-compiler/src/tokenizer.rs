#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub lexeme: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxToken {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriviaKind {
    Whitespace,
    LineComment,
    BlockComment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trivia {
    pub kind: TriviaKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Identifiers and literals
    Identifier,
    Number,
    String,
    StringStart,
    StringEnd,

    // Keywords
    Import,
    Class,
    Interface,
    Property,
    Function,
    Return,
    Var,
    Required,
    For,
    While,
    In,
    If,
    Else,
    Try,
    Catch,
    Finally,
    Continue,
    Break,
    Switch,
    Case,
    Default,
    Throw,
    New,
    True,
    False,
    Null,
    As,
    Public,
    Private,
    Remote,
    Package,
    Extends,
    Implements,
    Accessors,
    Abstract,
    Final,
    Static,
    Do,
    Assert,
    Param,
    Rethrow,
    Include,
    Not,

    // Punctuation
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dot,
    Semicolon,
    Colon,
    At,

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Equal,
    Less,
    Greater,
    Bang,
    Question,
    Ampersand,

    // Multi-char operators
    EqualEqual,
    EqualEqualEqual,
    BangEqual,
    BangEqualEqual,
    LessEqual,
    GreaterEqual,
    AmpAmp,
    PipePipe,
    EqualGreater,
    MinusGreater,
    QuestionColon,
    QuestionDot,
    ColonColon,
    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,
    PercentEqual,
    AmpEqual,
    Backslash,
    PlusPlus,
    MinusMinus,
    DotDot,
    DotDotDot,
    DotDotLess,
    GreaterDotDot,
    GreaterDotDotLess,
    // Bitwise operators
    BitwiseOr,
    BitwiseAnd,
    BitwiseXor,
    BitwiseComplement,
    BitwiseShiftLeft,
    BitwiseShiftRight,
    BitwiseUnsignedShiftRight,
    // Word operators
    Xor,
    Eqv,
    InstanceOf,
    CastAs,
    Contains,

    // Interpolation markers
    InterpStart,
    InterpEnd,
    ScriptStart,
    ScriptEnd,

    // Template tokens
    ContentText,
    ComponentName,
    ComponentOpen,
    ComponentClose,
    ComponentSelfClose,

    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LexerMode {
    DefaultScript,
    DefaultTemplate,
    TemplatePossibleComponent,
    TemplateComponentName,
    TemplateComponentMode,
    TemplateAttrValue,
    TemplateUnquotedValue,
    TemplateOutput,
    TemplateEndComponent,
    TemplateComment,
    TemplateScript,
}

impl std::fmt::Display for TokenKind {
    /// Human-readable token name for error messages.
    ///
    /// Punctuation renders as its literal glyph; keywords and other named
    /// kinds use lowercase, user-friendly names. Never leaks Rust Debug
    /// output like `LeftParen` or `Some(Identifier)`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s: &str = match self {
            // Punctuation — the literal source characters.
            TokenKind::LeftBrace => "{",
            TokenKind::RightBrace => "}",
            TokenKind::LeftParen => "(",
            TokenKind::RightParen => ")",
            TokenKind::LeftBracket => "[",
            TokenKind::RightBracket => "]",
            TokenKind::Comma => ",",
            TokenKind::Dot => ".",
            TokenKind::Semicolon => ";",
            TokenKind::Colon => ":",
            TokenKind::At => "@",
            TokenKind::Equal => "=",
            TokenKind::Less => "<",
            TokenKind::Greater => ">",
            TokenKind::Bang => "!",
            TokenKind::Question => "?",
            TokenKind::Ampersand => "&",

            // Operators (single- and multi-character).
            TokenKind::Plus => "+",
            TokenKind::Minus => "-",
            TokenKind::Star => "*",
            TokenKind::Slash => "/",
            TokenKind::Percent => "%",
            TokenKind::Caret => "^",
            TokenKind::EqualEqual => "==",
            TokenKind::EqualEqualEqual => "===",
            TokenKind::BangEqual => "!=",
            TokenKind::BangEqualEqual => "!==",
            TokenKind::LessEqual => "<=",
            TokenKind::GreaterEqual => ">=",
            TokenKind::AmpAmp => "&&",
            TokenKind::PipePipe => "||",
            TokenKind::EqualGreater => "=>",
            TokenKind::MinusGreater => "->",
            TokenKind::QuestionColon => "?:",
            TokenKind::QuestionDot => "?.",
            TokenKind::ColonColon => "::",
            TokenKind::PlusEqual => "+=",
            TokenKind::MinusEqual => "-=",
            TokenKind::StarEqual => "*=",
            TokenKind::SlashEqual => "/=",
            TokenKind::PercentEqual => "%=",
            TokenKind::Backslash => "\\",
            TokenKind::AmpEqual => "&=",
            TokenKind::PlusPlus => "++",
            TokenKind::MinusMinus => "--",
            TokenKind::DotDot => "..",
            TokenKind::DotDotDot => "...",
            TokenKind::DotDotLess => "..<",
            TokenKind::GreaterDotDot => ">..",
            TokenKind::GreaterDotDotLess => ">..<",
            TokenKind::BitwiseOr => "|",
            TokenKind::BitwiseAnd => "&",
            TokenKind::BitwiseXor => "^",
            TokenKind::BitwiseComplement => "~",
            TokenKind::BitwiseShiftLeft => "<<",
            TokenKind::BitwiseShiftRight => ">>",
            TokenKind::BitwiseUnsignedShiftRight => ">>>",

            // Literals / identifiers.
            TokenKind::Identifier => "identifier",
            TokenKind::Number => "number",
            TokenKind::String => "string",
            TokenKind::StringStart => "string start",
            TokenKind::StringEnd => "string end",

            // Keywords: lowercase keyword form, derived from source spelling.
            TokenKind::Import => "import",
            TokenKind::Class => "class",
            TokenKind::Interface => "interface",
            TokenKind::Property => "property",
            TokenKind::Function => "function",
            TokenKind::Return => "return",
            TokenKind::Var => "var",
            TokenKind::Required => "required",
            TokenKind::For => "for",
            TokenKind::While => "while",
            TokenKind::In => "in",
            TokenKind::If => "if",
            TokenKind::Else => "else",
            TokenKind::Try => "try",
            TokenKind::Catch => "catch",
            TokenKind::Finally => "finally",
            TokenKind::Continue => "continue",
            TokenKind::Break => "break",
            TokenKind::Switch => "switch",
            TokenKind::Case => "case",
            TokenKind::Default => "default",
            TokenKind::Throw => "throw",
            TokenKind::New => "new",
            TokenKind::True => "true",
            TokenKind::False => "false",
            TokenKind::Null => "null",
            TokenKind::As => "as",
            TokenKind::Public => "public",
            TokenKind::Private => "private",
            TokenKind::Remote => "remote",
            TokenKind::Package => "package",
            TokenKind::Extends => "extends",
            TokenKind::Implements => "implements",
            TokenKind::Accessors => "accessors",
            TokenKind::Abstract => "abstract",
            TokenKind::Final => "final",
            TokenKind::Static => "static",
            TokenKind::Do => "do",
            TokenKind::Assert => "assert",
            TokenKind::Param => "param",
            TokenKind::Rethrow => "rethrow",
            TokenKind::Include => "include",
            TokenKind::Not => "not",

            // Word operators.
            TokenKind::Xor => "xor",
            TokenKind::Eqv => "eqv",
            TokenKind::InstanceOf => "instanceof",
            TokenKind::CastAs => "castas",
            TokenKind::Contains => "contains",

            // Interpolation / template boundary tokens.
            TokenKind::InterpStart => "#{",
            TokenKind::InterpEnd => "}",
            TokenKind::ScriptStart => "<cfscript>",
            TokenKind::ScriptEnd => "</cfscript>",

            // Template-specific tokens.
            TokenKind::ContentText => "text",
            TokenKind::ComponentName => "tag name",
            TokenKind::ComponentOpen => "tag open",
            TokenKind::ComponentClose => "</",
            TokenKind::ComponentSelfClose => "/>",

            TokenKind::Eof => "end of input",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone)]
pub struct LexedSource<'a> {
    source: &'a str,
    tokens: Vec<SyntaxToken>,
    trivia: Vec<Trivia>,
}

impl<'a> LexedSource<'a> {
    pub fn source(&self) -> &'a str {
        self.source
    }

    pub fn tokens(&self) -> &[SyntaxToken] {
        &self.tokens
    }

    pub fn trivia(&self) -> &[Trivia] {
        &self.trivia
    }

    pub fn into_parts(self) -> (&'a str, Vec<SyntaxToken>, Vec<Trivia>) {
        (self.source, self.tokens, self.trivia)
    }

    pub fn text(&self, span: Span) -> &'a str {
        &self.source[span.start..span.end]
    }

    pub fn into_owned_tokens(self) -> Vec<Token> {
        self.tokens
            .into_iter()
            .map(|token| Token {
                kind: token.kind,
                span: token.span,
                lexeme: self.source[token.span.start..token.span.end].to_string(),
            })
            .collect()
    }
}

pub fn lex(source: &str) -> LexedSource<'_> {
    let mut lexer = Lexer::new(source, LexerMode::DefaultScript);
    lexer.lex()
}

pub fn lex_template(source: &str) -> LexedSource<'_> {
    let mut lexer = Lexer::new(source, LexerMode::DefaultTemplate);
    lexer.lex()
}

pub fn tokenize(source: &str) -> Vec<Token> {
    lex(source).into_owned_tokens()
}

pub fn tokenize_template(source: &str) -> Vec<Token> {
    lex_template(source).into_owned_tokens()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: u32,
    pub col: u32,
}

struct Lexer<'a> {
    source: &'a str,
    pos: usize,
    line: u32,
    col: u32,
    tokens: Vec<SyntaxToken>,
    trivia: Vec<Trivia>,
    mode_stack: Vec<LexerMode>,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str, initial_mode: LexerMode) -> Self {
        Self {
            source,
            pos: 0,
            line: 1,
            col: 1,
            tokens: Vec::new(),
            trivia: Vec::new(),
            mode_stack: vec![initial_mode],
        }
    }

    fn current_mode(&self) -> LexerMode {
        *self.mode_stack.last().unwrap_or(&LexerMode::DefaultScript)
    }

    fn push_mode(&mut self, mode: LexerMode) {
        self.mode_stack.push(mode);
    }

    fn pop_mode(&mut self) -> Option<LexerMode> {
        self.mode_stack.pop()
    }

    fn mode_stack_contains(&self, mode: LexerMode) -> bool {
        self.mode_stack.contains(&mode)
    }

    fn lex(&mut self) -> LexedSource<'a> {
        loop {
            if self.pos >= self.source.len() {
                break;
            }
            match self.current_mode() {
                LexerMode::DefaultScript => {
                    // Check for </bx:script> while in template script mode
                    if self.mode_stack_contains(LexerMode::TemplateScript) {
                        if self.source[self.pos..].starts_with("</bx:script>") {
                            let start = self.pos;
                            let start_line = self.line;
                            let start_col = self.col;
                            self.pos += 12;
                            self.col += 12;
                            self.push_token(
                                TokenKind::ScriptEnd,
                                start,
                                self.pos,
                                start_line,
                                start_col,
                            );
                            self.pop_mode(); // DefaultScript
                            self.pop_mode(); // TemplateScript
                            continue; // Back to template mode
                        }
                    }
                    // Check for ``` component island in script mode
                    if self.source[self.pos..].starts_with("```") {
                        self.pos += 3;
                        self.col += 3;
                        self.push_mode(LexerMode::DefaultTemplate);
                        continue;
                    }
                    self.skip_whitespace_and_comments();
                    if self.mode_stack_contains(LexerMode::TemplateScript)
                        && self.pos < self.source.len()
                        && self.source[self.pos..].starts_with("</bx:script>")
                    {
                        let start = self.pos;
                        let start_line = self.line;
                        let start_col = self.col;
                        self.pos += 12;
                        self.col += 12;
                        self.push_token(
                            TokenKind::ScriptEnd,
                            start,
                            self.pos,
                            start_line,
                            start_col,
                        );
                        self.pop_mode(); // DefaultScript
                        self.pop_mode(); // TemplateScript
                        continue;
                    }
                    if self.pos >= self.source.len() {
                        break;
                    }
                    let ch = self.current_char();
                    if is_ident_start(ch) {
                        self.tokenize_ident_or_bitwise();
                    } else if ch.is_ascii_digit() {
                        self.tokenize_number();
                    } else if ch == '"' || ch == '\'' {
                        self.tokenize_string(ch);
                    } else {
                        self.tokenize_operator_or_punct();
                    }
                }
                LexerMode::DefaultTemplate => {
                    self.tokenize_template_content();
                }
                LexerMode::TemplateOutput => {
                    self.tokenize_template_content();
                }
                _ => {
                    // Unhandled mode — skip character
                    self.advance();
                }
            }
        }
        LexedSource {
            source: self.source,
            tokens: std::mem::take(&mut self.tokens),
            trivia: std::mem::take(&mut self.trivia),
        }
    }

    fn tokenize_template_content(&mut self) {
        // Accumulate literal text until we hit < or #
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;

        while self.pos < self.source.len() {
            let ch = self.current_char();
            if ch == '<' || ch == '#' || ch == '`' {
                break;
            }
            self.advance();
        }

        if self.pos > start {
            self.push_token(
                TokenKind::ContentText,
                start,
                self.pos,
                start_line,
                start_col,
            );
        }

        // Handle the special character if we stopped at one
        if self.pos < self.source.len() {
            let ch = self.current_char();
            if ch == '`' {
                // Check for closing ``` only when coming from a component island
                if self.source[self.pos..].starts_with("```") {
                    self.pos += 3;
                    self.col += 3;
                    self.pop_mode(); // DefaultTemplate
                    return;
                }
                self.advance();
            } else if ch == '<' {
                let start = self.pos;
                self.advance(); // consume <
                let rest = &self.source[self.pos..];
                if rest.starts_with("bx:script") || rest.starts_with("bx:output") {
                    // Check which tag
                    let is_script = rest.starts_with("bx:script");
                    if is_script {
                        // <bx:script ...> — switch to script parsing mode
                        self.push_token(
                            TokenKind::ScriptStart,
                            start,
                            self.pos,
                            self.line,
                            self.col.saturating_sub(1),
                        );
                        self.push_mode(LexerMode::TemplateScript);
                        // Skip past opening > to find end of opening tag
                        while self.pos < self.source.len() && self.current_char() != '>' {
                            self.advance();
                        }
                        if self.pos < self.source.len() {
                            self.advance();
                        } // skip >
                        // Switch to script mode for body content
                        self.push_mode(LexerMode::DefaultScript);
                        return; // The main loop will now parse in DefaultScript
                    } else {
                        // <bx:output ...> — parse component normally, push output mode
                        self.push_mode(LexerMode::TemplateOutput);
                        self.pos += 3;
                        self.col += 3; // skip bx:
                        self.tokenize_component_name(start);
                        self.push_mode(LexerMode::TemplateOutput);
                        return;
                    }
                } else if rest.starts_with("bx:") {
                    self.push_mode(LexerMode::TemplateComponentName);
                    self.pos += 3;
                    self.col += 3; // skip bx:
                    self.tokenize_component_name(start);
                    return; // tokenize_component_name handles the rest
                } else if rest.starts_with("/bx:") {
                    let is_output_close = rest.starts_with("/bx:output");
                    self.push_mode(LexerMode::TemplateEndComponent);
                    self.pos += 4;
                    self.col += 4; // skip /bx:
                    self.tokenize_component_name(start);
                    if is_output_close {
                        self.pop_mode();
                    }
                    return;
                } else if rest.starts_with("!---") {
                    self.pos += 4;
                    self.col += 4;
                    while self.pos < self.source.len() {
                        if self.source[self.pos..].starts_with("--->") {
                            self.pos += 4;
                            self.col += 4;
                            break;
                        }
                        self.advance();
                    }
                } else {
                    self.push_token(
                        TokenKind::Less,
                        start,
                        self.pos,
                        self.line,
                        self.col.saturating_sub(1),
                    );
                }
            } else if ch == '#' {
                self.advance();
                if self.pos < self.source.len() && self.current_char() == '#' {
                    self.advance();
                    // Escaped hash — emit as ContentText with ##
                    let hash_start = self.pos - 2;
                    self.push_token(
                        TokenKind::ContentText,
                        hash_start,
                        self.pos,
                        self.line,
                        self.col.saturating_sub(2),
                    );
                } else if self.mode_stack_contains(LexerMode::TemplateOutput) {
                    // Expression interpolation in output mode: #expr#
                    // Parse the expression tokens inline
                    let interp_start = self.pos - 1;
                    self.push_token(
                        TokenKind::InterpStart,
                        interp_start,
                        self.pos,
                        self.line,
                        self.col.saturating_sub(1),
                    );
                    while self.pos < self.source.len() {
                        let c = self.current_char();
                        if c == '#' {
                            let end_start = self.pos;
                            let end_line = self.line;
                            let end_col = self.col;
                            self.advance();
                            if self.pos < self.source.len() && self.current_char() == '#' {
                                self.advance();
                                continue;
                            }
                            self.push_token(
                                TokenKind::InterpEnd,
                                end_start,
                                self.pos,
                                end_line,
                                end_col,
                            );
                            break;
                        } else if is_ident_start(c) {
                            self.tokenize_ident_or_bitwise();
                            continue;
                        } else if c.is_ascii_digit() {
                            self.tokenize_number();
                            continue;
                        } else if c == '"' || c == '\'' {
                            self.tokenize_string(c);
                            continue;
                        } else if !c.is_ascii_whitespace() {
                            let op_start = self.pos;
                            let op_start_line = self.line;
                            let op_start_col = self.col;
                            let kind = self.read_operator_or_punct_inline();
                            self.push_token(kind, op_start, self.pos, op_start_line, op_start_col);
                            continue;
                        } else {
                            self.advance();
                        }
                    }
                } else {
                    // Single # — emit as ContentText
                    self.push_token(
                        TokenKind::ContentText,
                        self.pos - 1,
                        self.pos,
                        self.line,
                        self.col.saturating_sub(1),
                    );
                }
            }
        }
    }

    fn current_char(&self) -> char {
        self.source[self.pos..].chars().next().unwrap_or('\0')
    }

    fn advance(&mut self) -> Option<char> {
        let ch = self.source[self.pos..].chars().next()?;
        self.pos += ch.len_utf8();
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn push_token(
        &mut self,
        kind: TokenKind,
        start: usize,
        end: usize,
        start_line: u32,
        start_col: u32,
    ) {
        self.tokens.push(SyntaxToken {
            kind,
            span: Span {
                start,
                end,
                line: start_line,
                col: start_col,
            },
        });
    }

    fn push_trivia(
        &mut self,
        kind: TriviaKind,
        start: usize,
        end: usize,
        start_line: u32,
        start_col: u32,
    ) {
        if start == end {
            return;
        }
        self.trivia.push(Trivia {
            kind,
            span: Span {
                start,
                end,
                line: start_line,
                col: start_col,
            },
        });
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            let remaining = &self.source[self.pos..];
            if remaining.starts_with("//") {
                let start = self.pos;
                let start_line = self.line;
                let start_col = self.col;
                self.advance();
                self.advance();
                while self.pos < self.source.len() && self.current_char() != '\n' {
                    self.advance();
                }
                self.push_trivia(
                    TriviaKind::LineComment,
                    start,
                    self.pos,
                    start_line,
                    start_col,
                );
            } else if remaining.starts_with("/*") {
                let start = self.pos;
                let start_line = self.line;
                let start_col = self.col;
                self.advance();
                self.advance();
                while self.pos < self.source.len() {
                    if self.source[self.pos..].starts_with("*/") {
                        self.advance();
                        self.advance();
                        break;
                    }
                    self.advance();
                }
                self.push_trivia(
                    TriviaKind::BlockComment,
                    start,
                    self.pos,
                    start_line,
                    start_col,
                );
            } else {
                let start = self.pos;
                let start_line = self.line;
                let start_col = self.col;
                while self.pos < self.source.len() {
                    let ch = self.current_char();
                    if ch != ' ' && ch != '\t' && ch != '\r' && ch != '\n' {
                        break;
                    }
                    self.advance();
                }
                if self.pos > start {
                    self.push_trivia(
                        TriviaKind::Whitespace,
                        start,
                        self.pos,
                        start_line,
                        start_col,
                    );
                } else {
                    break;
                }
            }
        }
    }

    fn tokenize_component_name(&mut self, tag_start: usize) {
        // Read component name
        let name_start = self.pos;
        let name_start_line = self.line;
        let name_start_col = self.col;
        while self.pos < self.source.len() {
            let ch = self.current_char();
            if ch.is_ascii_whitespace() || ch == '>' || ch == '/' {
                break;
            }
            self.advance();
        }
        self.push_token(
            TokenKind::ComponentName,
            name_start,
            self.pos,
            name_start_line,
            name_start_col,
        );
        self.push_mode(LexerMode::TemplateComponentMode);
        self.tokenize_component_rest();
        self.pop_mode(); // TemplateComponentMode
        self.pop_mode(); // TemplateComponentName or TemplateEndComponent
    }

    fn tokenize_component_rest(&mut self) {
        loop {
            if self.pos >= self.source.len() {
                break;
            }
            // Skip whitespace
            while self.pos < self.source.len() {
                let ch = self.current_char();
                if ch == ' ' || ch == '\t' || ch == '\r' || ch == '\n' {
                    self.advance();
                } else {
                    break;
                }
            }
            if self.pos >= self.source.len() {
                break;
            }
            let ch = self.current_char();
            if ch == '>' {
                self.advance();
                self.push_token(
                    TokenKind::ComponentClose,
                    self.pos - 1,
                    self.pos,
                    self.line,
                    self.col.saturating_sub(1),
                );
                break;
            }
            if ch == '/' {
                self.advance();
                if self.pos < self.source.len() && self.current_char() == '>' {
                    self.advance();
                    self.push_token(
                        TokenKind::ComponentSelfClose,
                        self.pos - 2,
                        self.pos,
                        self.line,
                        self.col.saturating_sub(2),
                    );
                    break;
                }
            }
            // Read attribute name
            let attr_start = self.pos;
            let attr_start_line = self.line;
            let attr_start_col = self.col;
            while self.pos < self.source.len() {
                let ch = self.current_char();
                if ch.is_ascii_whitespace() || ch == '=' || ch == '>' || ch == '/' {
                    break;
                }
                self.advance();
            }
            if self.pos > attr_start {
                self.push_token(
                    TokenKind::Identifier,
                    attr_start,
                    self.pos,
                    attr_start_line,
                    attr_start_col,
                );
            }
            // Handle = value
            if self.pos < self.source.len() && self.current_char() == '=' {
                let eq_start = self.pos;
                let eq_line = self.line;
                let eq_col = self.col;
                self.advance();
                self.push_token(TokenKind::Equal, eq_start, self.pos, eq_line, eq_col);
                while self.pos < self.source.len() {
                    let ch = self.current_char();
                    if ch == ' ' || ch == '\t' {
                        self.advance();
                    } else {
                        break;
                    }
                }
                if self.pos < self.source.len() {
                    let ch = self.current_char();
                    if ch == '"' || ch == '\'' {
                        self.tokenize_string(ch);
                    } else if ch == '#' {
                        self.advance();
                        while self.pos < self.source.len() && self.current_char() != '#' {
                            self.advance();
                        }
                        if self.pos < self.source.len() {
                            self.advance();
                        }
                    } else {
                        let val_start = self.pos;
                        let val_start_line = self.line;
                        let val_start_col = self.col;
                        while self.pos < self.source.len() {
                            let ch = self.current_char();
                            if ch.is_ascii_whitespace() || ch == '>' || ch == '/' {
                                break;
                            }
                            self.advance();
                        }
                        if self.pos > val_start {
                            self.push_token(
                                TokenKind::Identifier,
                                val_start,
                                self.pos,
                                val_start_line,
                                val_start_col,
                            );
                        }
                    }
                }
            }
        }
    }

    fn tokenize_ident_or_bitwise(&mut self) {
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // consume first char

        while self.pos < self.source.len() && is_ident_cont(self.current_char()) {
            self.advance();
        }
        let end = self.pos;

        // Check if this was just "b" followed by a bitwise operator
        let lexeme = &self.source[start..end];
        if lexeme == "b" && self.pos < self.source.len() {
            let rest = &self.source[self.pos..];
            if rest.starts_with(">>>") {
                self.pos += 3;
                self.col += 3;
                self.push_token(
                    TokenKind::BitwiseUnsignedShiftRight,
                    start,
                    self.pos,
                    start_line,
                    start_col,
                );
                return;
            }
            if rest.starts_with("<<") {
                self.pos += 2;
                self.col += 2;
                self.push_token(
                    TokenKind::BitwiseShiftLeft,
                    start,
                    self.pos,
                    start_line,
                    start_col,
                );
                return;
            }
            if rest.starts_with(">>") {
                self.pos += 2;
                self.col += 2;
                self.push_token(
                    TokenKind::BitwiseShiftRight,
                    start,
                    self.pos,
                    start_line,
                    start_col,
                );
                return;
            }
            let ch = self.current_char();
            if ch == '|' || ch == '&' || ch == '^' || ch == '~' {
                let kind = match ch {
                    '|' => TokenKind::BitwiseOr,
                    '&' => TokenKind::BitwiseAnd,
                    '^' => TokenKind::BitwiseXor,
                    '~' => TokenKind::BitwiseComplement,
                    _ => unreachable!(),
                };
                self.advance();
                self.push_token(kind, start, self.pos, start_line, start_col);
                return;
            }
        }

        let kind = keyword_or_ident(lexeme);
        self.push_token(kind, start, end, start_line, start_col);
    }

    fn tokenize_number(&mut self) {
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        self.advance(); // consume first digit

        while self.pos < self.source.len() && self.current_char().is_ascii_digit() {
            self.advance();
        }
        // Optional decimal part — only consume dot if followed by a digit
        if self.pos < self.source.len() && self.current_char() == '.' {
            // Peek ahead: only consume dot if next char is a digit
            if self.pos + 1 < self.source.len()
                && self.source.as_bytes()[self.pos + 1].is_ascii_digit()
            {
                self.advance(); // consume dot
                while self.pos < self.source.len() && self.current_char().is_ascii_digit() {
                    self.advance();
                }
            }
        }
        if self.pos < self.source.len() && matches!(self.current_char(), 'e' | 'E') {
            let exponent_start = self.pos;
            self.advance();
            if self.pos < self.source.len() && matches!(self.current_char(), '+' | '-') {
                self.advance();
            }
            let digits_start = self.pos;
            while self.pos < self.source.len() && self.current_char().is_ascii_digit() {
                self.advance();
            }
            if self.pos == digits_start {
                self.pos = exponent_start;
            }
        }
        let end = self.pos;
        self.push_token(TokenKind::Number, start, end, start_line, start_col);
    }

    fn tokenize_string(&mut self, quote: char) {
        let str_start = self.pos;
        let str_start_line = self.line;
        let str_start_col = self.col;

        self.advance(); // consume opening quote
        let xml_like_string = quote == '"' && self.source[self.pos..].trim_start().starts_with('<');

        loop {
            if self.pos >= self.source.len() {
                break;
            }
            let ch = self.current_char();
            let xml_attribute_quote = if xml_like_string {
                let mut lookahead = self.pos + ch.len_utf8();
                let mut result = false;
                while lookahead < self.source.len() {
                    let next = self.source[lookahead..].chars().next().unwrap();
                    if !next.is_whitespace() {
                        let after_next = lookahead + next.len_utf8();
                        result = next.is_ascii_alphanumeric()
                            || next == '>'
                            || (next == '/' && self.source[after_next..].starts_with('>'));
                        break;
                    }
                    lookahead += next.len_utf8();
                }
                result
            } else {
                false
            };
            if ch == quote && !xml_attribute_quote {
                self.advance();
                if self.pos < self.source.len() && self.current_char() == quote {
                    self.advance();
                    continue;
                }
                break; // closing quote
            }
            if ch == '#' {
                let mut lookahead = self.pos + 1;
                let mut has_closing_hash = false;
                while lookahead < self.source.len() {
                    let next = self.source[lookahead..].chars().next().unwrap();
                    if next == quote {
                        break;
                    }
                    if next == '#' {
                        has_closing_hash = true;
                        break;
                    }
                    lookahead += next.len_utf8();
                }
                if !has_closing_hash {
                    self.advance();
                    continue;
                }
                self.advance();
                if self.pos < self.source.len() && self.current_char() == '#' {
                    self.advance();
                    continue;
                }
                // Interpolation start — skip the expression inside
                // (it will be re-parsed by parse_string_content)
                let mut depth = 1;
                while self.pos < self.source.len() && depth > 0 {
                    let c = self.current_char();
                    if c == '"' {
                        self.advance();
                        while self.pos < self.source.len() && self.current_char() != '"' {
                            self.advance();
                        }
                        if self.pos < self.source.len() {
                            self.advance();
                        }
                        continue;
                    }
                    if c == '\'' {
                        self.advance();
                        while self.pos < self.source.len() && self.current_char() != '\'' {
                            self.advance();
                        }
                        if self.pos < self.source.len() {
                            self.advance();
                        }
                        continue;
                    }
                    if c == '#' {
                        self.advance();
                        if self.pos < self.source.len() && self.current_char() == '#' {
                            self.advance();
                            continue;
                        }
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                        depth += 1;
                        continue;
                    }
                    self.advance();
                }
                continue;
            }
            self.advance();
        }

        let str_end = self.pos;

        self.push_token(
            TokenKind::String,
            str_start,
            str_end,
            str_start_line,
            str_start_col,
        );
    }

    fn tokenize_operator_or_punct(&mut self) {
        let start = self.pos;
        let start_line = self.line;
        let start_col = self.col;
        let ch = self.advance().unwrap();
        let kind = self.read_op_kind(ch);
        self.push_token(kind, start, self.pos, start_line, start_col);
    }

    fn read_operator_or_punct_inline(&mut self) -> TokenKind {
        let ch = self.advance().unwrap();
        self.read_op_kind(ch)
    }

    fn read_op_kind(&mut self, ch: char) -> TokenKind {
        let kind = match ch {
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ',' => TokenKind::Comma,
            '.' => self.tokenize_dots(),
            ';' => TokenKind::Semicolon,
            ':' => self.tokenize_colon(),
            '@' => TokenKind::At,
            '+' => self.tokenize_plus(),
            '-' => self.tokenize_minus(),
            '*' => self.tokenize_star(),
            '/' => self.tokenize_slash(),
            '\\' => self.tokenize_backslash(),
            '%' => self.tokenize_percent(),
            '^' => TokenKind::Caret,
            '=' => self.tokenize_equal(),
            '<' => self.tokenize_less(),
            '>' => self.tokenize_greater(),
            '!' => self.tokenize_bang(),
            '?' => self.tokenize_question(),
            '&' => self.tokenize_ampersand(),
            '|' => self.tokenize_pipe(),
            _ => TokenKind::Eof,
        };
        kind
    }

    fn next_char_matches(&self, expected: char) -> bool {
        self.pos < self.source.len() && self.current_char() == expected
    }

    fn tokenize_dots(&mut self) -> TokenKind {
        if self.next_char_matches('.') {
            self.advance();
            if self.next_char_matches('.') {
                self.advance();
                TokenKind::DotDotDot
            } else if self.next_char_matches('<') {
                self.advance();
                TokenKind::DotDotLess
            } else {
                TokenKind::DotDot
            }
        } else {
            TokenKind::Dot
        }
    }

    fn tokenize_colon(&mut self) -> TokenKind {
        if self.next_char_matches(':') {
            self.advance();
            TokenKind::ColonColon
        } else {
            TokenKind::Colon
        }
    }

    fn tokenize_equal(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            if self.next_char_matches('=') {
                self.advance();
                TokenKind::EqualEqualEqual
            } else {
                TokenKind::EqualEqual
            }
        } else if self.next_char_matches('>') {
            self.advance();
            TokenKind::EqualGreater
        } else {
            TokenKind::Equal
        }
    }

    fn tokenize_plus(&mut self) -> TokenKind {
        if self.next_char_matches('+') {
            self.advance();
            TokenKind::PlusPlus
        } else if self.next_char_matches('=') {
            self.advance();
            TokenKind::PlusEqual
        } else {
            TokenKind::Plus
        }
    }

    fn tokenize_minus(&mut self) -> TokenKind {
        if self.next_char_matches('-') {
            self.advance();
            TokenKind::MinusMinus
        } else if self.next_char_matches('=') {
            self.advance();
            TokenKind::MinusEqual
        } else if self.next_char_matches('>') {
            self.advance();
            TokenKind::MinusGreater
        } else {
            TokenKind::Minus
        }
    }

    fn tokenize_star(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            TokenKind::StarEqual
        } else {
            TokenKind::Star
        }
    }

    fn tokenize_slash(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            TokenKind::SlashEqual
        } else {
            TokenKind::Slash
        }
    }

    fn tokenize_backslash(&mut self) -> TokenKind {
        TokenKind::Backslash
    }

    fn tokenize_percent(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            TokenKind::PercentEqual
        } else {
            TokenKind::Percent
        }
    }

    fn tokenize_less(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            TokenKind::LessEqual
        } else {
            TokenKind::Less
        }
    }

    fn tokenize_greater(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            TokenKind::GreaterEqual
        } else if self.source[self.pos..].starts_with("..") {
            // >.. or >..<
            self.advance(); // first dot
            self.advance(); // second dot
            if self.next_char_matches('<') {
                self.advance();
                TokenKind::GreaterDotDotLess
            } else {
                TokenKind::GreaterDotDot
            }
        } else {
            TokenKind::Greater
        }
    }

    fn tokenize_bang(&mut self) -> TokenKind {
        if self.next_char_matches('=') {
            self.advance();
            if self.next_char_matches('=') {
                self.advance();
                TokenKind::BangEqualEqual
            } else {
                TokenKind::BangEqual
            }
        } else {
            TokenKind::Bang
        }
    }

    fn tokenize_question(&mut self) -> TokenKind {
        if self.next_char_matches('.') {
            self.advance();
            TokenKind::QuestionDot
        } else if self.next_char_matches(':') {
            self.advance();
            TokenKind::QuestionColon
        } else {
            TokenKind::Question
        }
    }

    fn tokenize_ampersand(&mut self) -> TokenKind {
        if self.next_char_matches('&') {
            self.advance();
            TokenKind::AmpAmp
        } else if self.next_char_matches('=') {
            self.advance();
            TokenKind::AmpEqual
        } else {
            TokenKind::Ampersand
        }
    }

    fn tokenize_pipe(&mut self) -> TokenKind {
        if self.next_char_matches('|') {
            self.advance();
            TokenKind::PipePipe
        } else {
            // Single | isn't used in BoxLang; treat as PipePipe for now
            TokenKind::PipePipe
        }
    }
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_' || ch == '$'
}

fn is_ident_cont(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '$'
}

fn keyword_or_ident(lexeme: &str) -> TokenKind {
    match lexeme.to_ascii_lowercase().as_str() {
        "import" => TokenKind::Import,
        "class" => TokenKind::Class,
        "interface" => TokenKind::Interface,
        "property" => TokenKind::Property,
        "function" => TokenKind::Function,
        "return" => TokenKind::Return,
        "var" => TokenKind::Var,
        "required" => TokenKind::Required,
        "for" => TokenKind::For,
        "while" => TokenKind::While,
        "in" => TokenKind::In,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "try" => TokenKind::Try,
        "catch" => TokenKind::Catch,
        "finally" => TokenKind::Finally,
        "continue" => TokenKind::Continue,
        "break" => TokenKind::Break,
        "switch" => TokenKind::Switch,
        "case" => TokenKind::Case,
        "default" => TokenKind::Default,
        "throw" => TokenKind::Throw,
        "new" => TokenKind::New,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "null" => TokenKind::Null,
        "as" => TokenKind::As,
        "public" => TokenKind::Public,
        "private" => TokenKind::Private,
        "remote" => TokenKind::Remote,
        "package" => TokenKind::Package,
        "extends" => TokenKind::Extends,
        "implements" => TokenKind::Implements,
        "accessors" => TokenKind::Accessors,
        "abstract" => TokenKind::Abstract,
        "final" => TokenKind::Final,
        "static" => TokenKind::Static,
        "do" => TokenKind::Do,
        "assert" => TokenKind::Assert,
        "param" => TokenKind::Param,
        "rethrow" => TokenKind::Rethrow,
        "include" => TokenKind::Include,
        "not" => TokenKind::Not,
        "xor" => TokenKind::Xor,
        "eqv" => TokenKind::Eqv,
        "and" => TokenKind::AmpAmp,
        "or" => TokenKind::PipePipe,
        "eq" | "equal" | "is" => TokenKind::EqualEqual,
        "neq" | "notequal" | "not_equal" => TokenKind::BangEqual,
        "gt" => TokenKind::Greater,
        "gte" | "ge" => TokenKind::GreaterEqual,
        "lt" => TokenKind::Less,
        "lte" | "le" => TokenKind::LessEqual,
        "instanceof" => TokenKind::InstanceOf,
        "castas" => TokenKind::CastAs,
        "contains" => TokenKind::Contains,
        _ => TokenKind::Identifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_source() {
        let tokens = tokenize("");
        assert!(tokens.is_empty());
    }

    #[test]
    fn single_identifier() {
        let tokens = tokenize("foo");
        assert_eq!(tokens.len(), 1);
        assert!(matches!(tokens[0].kind, TokenKind::Identifier));
        assert_eq!(tokens[0].lexeme, "foo");
        assert_eq!(tokens[0].span.line, 1);
        assert_eq!(tokens[0].span.col, 1);
    }

    #[test]
    fn keyword_function() {
        let tokens = tokenize("function");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Function);
        assert_eq!(tokens[0].lexeme, "function");
    }

    #[test]
    fn keyword_vs_identifier_boundary() {
        let tokens = tokenize("functionfoo");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[0].lexeme, "functionfoo");
    }

    #[test]
    fn all_keywords() {
        let keywords = [
            ("import", TokenKind::Import),
            ("class", TokenKind::Class),
            ("interface", TokenKind::Interface),
            ("property", TokenKind::Property),
            ("function", TokenKind::Function),
            ("return", TokenKind::Return),
            ("var", TokenKind::Var),
            ("required", TokenKind::Required),
            ("for", TokenKind::For),
            ("while", TokenKind::While),
            ("in", TokenKind::In),
            ("if", TokenKind::If),
            ("else", TokenKind::Else),
            ("try", TokenKind::Try),
            ("catch", TokenKind::Catch),
            ("finally", TokenKind::Finally),
            ("continue", TokenKind::Continue),
            ("break", TokenKind::Break),
            ("switch", TokenKind::Switch),
            ("case", TokenKind::Case),
            ("default", TokenKind::Default),
            ("throw", TokenKind::Throw),
            ("new", TokenKind::New),
            ("true", TokenKind::True),
            ("false", TokenKind::False),
            ("null", TokenKind::Null),
            ("as", TokenKind::As),
            ("public", TokenKind::Public),
            ("private", TokenKind::Private),
            ("extends", TokenKind::Extends),
            ("implements", TokenKind::Implements),
            ("accessors", TokenKind::Accessors),
        ];
        for (src, expected_kind) in keywords {
            let tokens = tokenize(src);
            assert_eq!(tokens.len(), 1, "failed for '{src}'");
            assert_eq!(tokens[0].kind, expected_kind, "failed for '{src}'");
            assert_eq!(tokens[0].lexeme, src, "failed for '{src}'");
        }
    }

    #[test]
    fn keyword_with_trailing_space() {
        let tokens = tokenize("return ");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Return);
    }

    #[test]
    fn integer_literal() {
        let tokens = tokenize("42");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].lexeme, "42");
    }

    #[test]
    fn decimal_literal() {
        let tokens = tokenize("3.14");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Number);
        assert_eq!(tokens[0].lexeme, "3.14");
    }

    #[test]
    fn negative_number_is_minus_then_number() {
        let tokens = tokenize("-42");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::Minus);
        assert_eq!(tokens[1].kind, TokenKind::Number);
    }

    #[test]
    fn basic_operators_and_punctuation() {
        let cases = [
            ("{", TokenKind::LeftBrace),
            ("}", TokenKind::RightBrace),
            ("(", TokenKind::LeftParen),
            (")", TokenKind::RightParen),
            ("[", TokenKind::LeftBracket),
            ("]", TokenKind::RightBracket),
            (",", TokenKind::Comma),
            (".", TokenKind::Dot),
            (";", TokenKind::Semicolon),
            (":", TokenKind::Colon),
            ("@", TokenKind::At),
            ("+", TokenKind::Plus),
            ("-", TokenKind::Minus),
            ("*", TokenKind::Star),
            ("/", TokenKind::Slash),
            ("%", TokenKind::Percent),
            ("^", TokenKind::Caret),
            ("=", TokenKind::Equal),
            ("<", TokenKind::Less),
            (">", TokenKind::Greater),
            ("!", TokenKind::Bang),
            ("?", TokenKind::Question),
            ("&", TokenKind::Ampersand),
        ];
        for (src, expected) in cases {
            let tokens = tokenize(src);
            assert_eq!(tokens.len(), 1, "failed for '{src}'");
            assert_eq!(tokens[0].kind, expected, "failed for '{src}'");
            assert_eq!(tokens[0].lexeme, src, "failed for '{src}'");
        }
    }

    #[test]
    fn multi_char_operators() {
        let cases = [
            ("==", TokenKind::EqualEqual),
            ("!=", TokenKind::BangEqual),
            ("<=", TokenKind::LessEqual),
            (">=", TokenKind::GreaterEqual),
            ("&&", TokenKind::AmpAmp),
            ("||", TokenKind::PipePipe),
            ("=>", TokenKind::EqualGreater),
            ("->", TokenKind::MinusGreater),
            ("?:", TokenKind::QuestionColon),
            ("?.", TokenKind::QuestionDot),
            ("::", TokenKind::ColonColon),
            ("+=", TokenKind::PlusEqual),
            ("-=", TokenKind::MinusEqual),
            ("*=", TokenKind::StarEqual),
            ("/=", TokenKind::SlashEqual),
            ("%=", TokenKind::PercentEqual),
            ("++", TokenKind::PlusPlus),
            ("--", TokenKind::MinusMinus),
            ("..", TokenKind::DotDot),
        ];
        for (src, expected) in cases {
            let tokens = tokenize(src);
            assert_eq!(tokens.len(), 1, "failed for '{src}'");
            assert_eq!(tokens[0].kind, expected, "failed for '{src}'");
            assert_eq!(tokens[0].lexeme, src, "failed for '{src}'");
        }
    }

    #[test]
    fn line_comment_skipped() {
        let tokens = tokenize("// this is a comment\nfoo");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[0].lexeme, "foo");
        assert_eq!(tokens[0].span.line, 2);
    }

    #[test]
    fn block_comment_skipped() {
        let tokens = tokenize("/* comment */ foo");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[0].lexeme, "foo");
    }

    #[test]
    fn empty_double_quoted_string() {
        let tokens = tokenize("\"\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, "\"\"");
    }

    #[test]
    fn simple_double_quoted_string() {
        let tokens = tokenize("\"hello\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, "\"hello\"");
    }

    #[test]
    fn simple_single_quoted_string() {
        let tokens = tokenize("'hello'");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, "'hello'");
    }

    #[test]
    fn escaped_double_quote_in_string() {
        let tokens = tokenize("\"he\"\"llo\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].lexeme, "\"he\"\"llo\"");
    }

    #[test]
    fn escaped_single_quote_in_string() {
        let tokens = tokenize("'he''llo'");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].lexeme, "'he''llo'");
    }

    #[test]
    fn escaped_hash_in_string() {
        let tokens = tokenize("\"##\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].lexeme, "\"##\"");
    }

    #[test]
    fn string_with_interpolation() {
        let tokens = tokenize("\"hello #name#\"");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::String);
        assert_eq!(tokens[0].lexeme, "\"hello #name#\"");
    }

    #[test]
    fn multiple_tokens_with_whitespace() {
        let tokens = tokenize("var x = 42");
        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].kind, TokenKind::Var);
        assert_eq!(tokens[1].kind, TokenKind::Identifier);
        assert_eq!(tokens[1].lexeme, "x");
        assert_eq!(tokens[2].kind, TokenKind::Equal);
        assert_eq!(tokens[3].kind, TokenKind::Number);
    }

    #[test]
    fn phase2_keywords() {
        let keywords = [
            ("do", TokenKind::Do),
            ("assert", TokenKind::Assert),
            ("param", TokenKind::Param),
            ("rethrow", TokenKind::Rethrow),
            ("include", TokenKind::Include),
            ("not", TokenKind::Not),
            ("abstract", TokenKind::Abstract),
            ("final", TokenKind::Final),
            ("static", TokenKind::Static),
            ("remote", TokenKind::Remote),
            ("package", TokenKind::Package),
        ];
        for (src, expected_kind) in keywords {
            let tokens = tokenize(src);
            assert_eq!(tokens.len(), 1, "failed for '{src}'");
            assert_eq!(tokens[0].kind, expected_kind, "failed for '{src}'");
            assert_eq!(tokens[0].lexeme, src, "failed for '{src}'");
        }
    }

    #[test]
    fn range_operators() {
        let cases = [
            ("..", TokenKind::DotDot),
            ("..<", TokenKind::DotDotLess),
            (">..", TokenKind::GreaterDotDot),
            (">..<", TokenKind::GreaterDotDotLess),
            ("...", TokenKind::DotDotDot),
        ];
        for (src, expected) in cases {
            let tokens = tokenize(src);
            assert_eq!(tokens.len(), 1, "failed for '{src}'");
            assert_eq!(tokens[0].kind, expected, "failed for '{src}'");
            assert_eq!(tokens[0].lexeme, src, "failed for '{src}'");
        }
    }

    #[test]
    fn safe_nav_and_elvis() {
        let tokens = tokenize("a?.b ?: c");
        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].kind, TokenKind::Identifier);
        assert_eq!(tokens[0].lexeme, "a");
        assert_eq!(tokens[1].kind, TokenKind::QuestionDot);
        assert_eq!(tokens[2].kind, TokenKind::Identifier);
        assert_eq!(tokens[2].lexeme, "b");
        assert_eq!(tokens[3].kind, TokenKind::QuestionColon);
        assert_eq!(tokens[4].kind, TokenKind::Identifier);
    }

    #[test]
    fn dot_dot_dot_spread() {
        let tokens = tokenize("...");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::DotDotDot);
        assert_eq!(tokens[0].lexeme, "...");
    }

    #[test]
    fn template_plain_text() {
        let tokens = tokenize_template("hello");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::ContentText);
        assert_eq!(tokens[0].lexeme, "hello");
    }

    #[test]
    fn template_text_with_whitespace() {
        let tokens = tokenize_template("hello world");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::ContentText);
        assert_eq!(tokens[0].lexeme, "hello world");
    }

    #[test]
    fn template_hash_hash_escape() {
        let tokens = tokenize_template("##");
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].kind, TokenKind::ContentText);
        assert_eq!(tokens[0].lexeme, "##");
    }

    #[test]
    fn template_comment_skipped() {
        let tokens = tokenize_template("before<!--- comment --->after");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::ContentText);
        assert_eq!(tokens[0].lexeme, "before");
        assert_eq!(tokens[1].kind, TokenKind::ContentText);
        assert_eq!(tokens[1].lexeme, "after");
    }

    #[test]
    fn template_component_basic() {
        let tokens = tokenize_template("<bx:set>");
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].kind, TokenKind::ComponentName);
        assert_eq!(tokens[0].lexeme, "set");
        assert_eq!(tokens[1].kind, TokenKind::ComponentClose);
    }

    #[test]
    fn template_component_with_attr() {
        let tokens = tokenize_template("<bx:set x = \"hello\">");
        assert_eq!(tokens[0].kind, TokenKind::ComponentName);
        assert_eq!(tokens[0].lexeme, "set");
        assert_eq!(tokens[1].kind, TokenKind::Identifier); // x
        assert_eq!(tokens[2].kind, TokenKind::Equal);
        assert_eq!(tokens[3].kind, TokenKind::String); // "hello"
        assert_eq!(tokens[4].kind, TokenKind::ComponentClose);
    }

    #[test]
    fn template_component_self_close() {
        let tokens = tokenize_template("<bx:set x = 10 />");
        assert_eq!(tokens[0].kind, TokenKind::ComponentName);
        assert_eq!(tokens.last().unwrap().kind, TokenKind::ComponentSelfClose);
    }

    #[test]
    fn template_closing_tag() {
        let tokens = tokenize_template("</bx:if>");
        assert_eq!(tokens[0].kind, TokenKind::ComponentName);
        assert_eq!(tokens[0].lexeme, "if");
        assert_eq!(tokens[1].kind, TokenKind::ComponentClose);
    }

    #[test]
    fn template_script_island_tokens() {
        let tokens = tokenize_template("<bx:script>var x = 1;</bx:script>after");
        assert!(tokens.iter().any(|t| t.lexeme == "var"));
        assert!(tokens.iter().any(|t| t.lexeme == "x"));
        assert!(tokens.iter().any(|t| t.lexeme == "after"));
    }

    #[test]
    fn template_output_markers_and_script_markers() {
        let tokens = tokenize_template(
            "<bx:script>var name = 1;</bx:script><bx:output>Hello #name#! ##</bx:output>",
        );
        assert!(tokens.iter().any(|t| t.kind == TokenKind::ScriptStart));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::ScriptEnd));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::InterpStart));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::InterpEnd));
        assert!(
            tokens
                .iter()
                .any(|t| t.kind == TokenKind::ComponentName && t.lexeme == "output")
        );
    }
}
