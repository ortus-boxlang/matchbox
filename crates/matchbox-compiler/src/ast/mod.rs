#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub line: u32,
}

impl Statement {
    pub fn new(kind: StatementKind, line: u32) -> Self {
        Self { kind, line }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionParam {
    pub name: String,
    pub type_name: Option<String>,
    pub required: bool,
    pub default_value: Option<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    pub name: String,
    pub args: Vec<Argument>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Import(String),
    ClassDecl {
        name: String,
        extends: Option<String>,
        accessors: bool,
        implements: Vec<String>,
        members: Vec<ClassMember>,
    },
    InterfaceDecl {
        name: String,
        members: Vec<Statement>,
    },
    FunctionDecl {
        name: String,
        attributes: Vec<Attribute>,
        access_modifier: Option<String>,
        return_type: Option<String>,
        params: Vec<FunctionParam>,
        body: FunctionBody,
    },
    ForLoop {
        item: String,
        index: Option<String>,
        collection: Expression,
        body: Vec<Statement>,
    },
    ForClassic {
        init: Option<Box<Statement>>,
        condition: Option<Expression>,
        update: Option<Expression>,
        body: Vec<Statement>,
    },
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
    Return(Option<Expression>),
    Throw(Option<Expression>),
    TryCatch {
        try_branch: Vec<Statement>,
        catches: Vec<CatchBlock>,
        finally_branch: Option<Vec<Statement>>,
    },
    VariableDecl {
        name: String,
        value: Expression,
    },
    Expression(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CatchBlock {
    pub exception_var: String,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Property(String),
    Statement(Statement),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub line: u32,
}

impl Expression {
    pub fn new(kind: ExpressionKind, line: u32) -> Self {
        Self { kind, line }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Argument {
    pub name: Option<String>,
    pub value: Expression,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExpressionKind {
    New {
        class_path: String,
        args: Vec<Argument>,
    },
    Assignment {
        target: AssignmentTarget,
        value: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    FunctionCall {
        base: Box<Expression>,
        args: Vec<Argument>,
    },
    ArrayAccess {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    MemberAccess {
        base: Box<Expression>,
        member: String,
    },
    Prefix {
        operator: String,
        target: AssignmentTarget,
    },
    Postfix {
        base: Box<Expression>,
        operator: String,
    },
    Identifier(String),
    Literal(Literal),
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentTarget {
    Identifier(String),
    Member {
        base: Box<Expression>,
        member: String,
    },
    Index {
        base: Box<Expression>,
        index: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(Vec<StringPart>),
    Number(f64),
    Boolean(bool),
    Null,
    Array(Vec<Expression>),
    Struct(Vec<(Expression, Expression)>),
    Function {
        params: Vec<FunctionParam>,
        body: FunctionBody,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionBody {
    Block(Vec<Statement>),
    Expression(Box<Expression>),
    Abstract,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringPart {
    Text(String),
    Expression(Expression),
}
