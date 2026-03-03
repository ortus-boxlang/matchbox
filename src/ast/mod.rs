#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    FunctionDecl {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
    },
    ForLoop {
        item: String,
        collection: Expression,
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
        name: String,
        args: Vec<Expression>,
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
}
