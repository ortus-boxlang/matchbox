#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
    },
    ForLoop {
        item: String,
        index: Option<String>,
        collection: Expression,
        body: Vec<Statement>,
    },
    ForClassic {
        init: Option<Expression>,
        condition: Option<Expression>,
        update: Option<Expression>,
        body: Vec<Statement>,
    },
    If {
        condition: Expression,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
    Expression(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Assignment {
        target: String,
        value: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        operator: String,
        right: Box<Expression>,
    },
    FunctionCall {
        base: Box<Expression>,
        args: Vec<Expression>,
    },
    ArrayAccess {
        base: Box<Expression>,
        index: Box<Expression>,
    },
    Identifier(String),
    Literal(Literal),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
    Array(Vec<Expression>),
}
