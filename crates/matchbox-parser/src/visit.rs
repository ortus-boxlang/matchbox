//! Immutable, native syntax snapshots. Traversal policy/analysis belongs to callers.
use crate::{ast::*, parser::{self, ParseError}, tokenizer::Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Kind {
    Script=1, Variable, Expression, Number, Function, Block, Return, Closure, Lambda,
    Class, Interface, Property, Import, ForIn, For, While, DoWhile, If, Switch, Case,
    Throw, Break, Continue, Rethrow, Assert, Param, Include, Destructure, Output, Try,
    Catch, Component, Parameter, Annotation, New, Assignment, Target, Binary, Unary,
    Ternary, Elvis, Call, Argument, Index, Member, SafeMember, Prefix, Postfix, Spread,
    Identifier, String, Boolean, Null, Array, Struct,
}
impl Kind {
    pub const ALL: &'static [Self] = &[Self::Script,Self::Variable,Self::Expression,Self::Number,Self::Function,Self::Block,Self::Return,Self::Closure,Self::Lambda,Self::Class,Self::Interface,Self::Property,Self::Import,Self::ForIn,Self::For,Self::While,Self::DoWhile,Self::If,Self::Switch,Self::Case,Self::Throw,Self::Break,Self::Continue,Self::Rethrow,Self::Assert,Self::Param,Self::Include,Self::Destructure,Self::Output,Self::Try,Self::Catch,Self::Component,Self::Parameter,Self::Annotation,Self::New,Self::Assignment,Self::Target,Self::Binary,Self::Unary,Self::Ternary,Self::Elvis,Self::Call,Self::Argument,Self::Index,Self::Member,Self::SafeMember,Self::Prefix,Self::Postfix,Self::Spread,Self::Identifier,Self::String,Self::Boolean,Self::Null,Self::Array,Self::Struct];
    pub fn name(self) -> &'static str {
        match self {
            Self::Script=>"Script",Self::Variable=>"Variable",Self::Expression=>"Expression",Self::Number=>"Number",Self::Function=>"Function",Self::Block=>"Block",Self::Return=>"Return",Self::Closure=>"Closure",Self::Lambda=>"Lambda",Self::Class=>"Class",Self::Interface=>"Interface",Self::Property=>"Property",Self::Import=>"Import",Self::ForIn=>"ForIn",Self::For=>"For",Self::While=>"While",Self::DoWhile=>"DoWhile",Self::If=>"If",Self::Switch=>"Switch",Self::Case=>"Case",Self::Throw=>"Throw",Self::Break=>"Break",Self::Continue=>"Continue",Self::Rethrow=>"Rethrow",Self::Assert=>"Assert",Self::Param=>"Param",Self::Include=>"Include",Self::Destructure=>"Destructure",Self::Output=>"Output",Self::Try=>"Try",Self::Catch=>"Catch",Self::Component=>"Component",Self::Parameter=>"Parameter",Self::Annotation=>"Annotation",Self::New=>"New",Self::Assignment=>"Assignment",Self::Target=>"Target",Self::Binary=>"Binary",Self::Unary=>"Unary",Self::Ternary=>"Ternary",Self::Elvis=>"Elvis",Self::Call=>"Call",Self::Argument=>"Argument",Self::Index=>"Index",Self::Member=>"Member",Self::SafeMember=>"SafeMember",Self::Prefix=>"Prefix",Self::Postfix=>"Postfix",Self::Spread=>"Spread",Self::Identifier=>"Identifier",Self::String=>"String",Self::Boolean=>"Boolean",Self::Null=>"Null",Self::Array=>"Array",Self::Struct=>"Struct",
        }
    }
}
#[derive(Debug, Clone)]
pub enum AttributeValue { Text(String), Flag(bool), Names(Vec<String>), Number(f64) }
#[derive(Debug)]
pub struct Node {
    pub kind: Kind,
    pub span: Option<Span>,
    pub incomplete: bool,
    pub parent: usize,
    pub edge: &'static str,
    pub children: Vec<usize>,
    pub body: Option<usize>,
    pub name: Option<String>,
    pub return_type: Option<String>,
    pub body_span: Option<Span>,
    pub attributes: Vec<(&'static str, AttributeValue)>,
}
#[derive(Debug)]
pub struct Snapshot {
    pub source: String,
    pub statements: Vec<Statement>,
    pub issues: Vec<ParseError>,
    pub recover: bool,
    pub include_tokens: bool,
    pub nodes: Vec<Node>,
}
impl Snapshot {
    pub fn parse(source: String, filename: Option<&str>, recover: bool, include_tokens: bool) -> anyhow::Result<Self> {
        let (statements,issues)=if recover {parser::parse_recover(&source,filename)} else {(parser::parse(&source,filename)?,Vec::new())};
        let mut nodes=Vec::new();
        let root=add(&mut nodes,Kind::Script,Some(Span::at(&source,0,source.len())),0,"root",None,false);
        for statement in &statements {stmt(&mut nodes,statement,root,"statements");}
        Ok(Self{source,statements,issues,recover,include_tokens,nodes})
    }
}
fn add(nodes:&mut Vec<Node>,kind:Kind,span:Option<Span>,parent:usize,edge:&'static str,name:Option<&str>,incomplete:bool)->usize {
    let id=nodes.len();nodes.push(Node{kind,span,incomplete,parent,edge,children:Vec::new(),body:None,name:name.map(str::to_owned),return_type:None,body_span:None,attributes:Vec::new()});
    if id!=0 {nodes[parent].children.push(id);} id
}
fn block(nodes:&mut Vec<Node>,body:&[Statement],parent:usize,edge:&'static str,span:Option<Span>)->usize {
    let id=add(nodes,Kind::Block,span,parent,edge,None,false);
    for statement in body {stmt(nodes,statement,id,"statements");} id
}
fn function_body(nodes:&mut Vec<Node>,body:&FunctionBody,parent:usize,span:Option<Span>) {
    let id=match body {
        FunctionBody::Block(body)|FunctionBody::Component(body)=>Some(block(nodes,body,parent,"body",span)),
        FunctionBody::Expression(value)=>Some(expr(nodes,value,parent,"body")),FunctionBody::Abstract=>None,
    };nodes[parent].body=id;
}
fn params(nodes:&mut Vec<Node>,params:&[FunctionParam],parent:usize) {
    for param in params {let id=add(nodes,Kind::Parameter,None,parent,"parameters",Some(&param.name),false);nodes[id].attributes.push(("required",AttributeValue::Flag(param.required)));if let Some(t)=&param.type_name{nodes[id].attributes.push(("type",AttributeValue::Text(t.clone())));}if let Some(value)=&param.default_value{expr(nodes,value,id,"default");}}
}
fn attributes(nodes:&mut Vec<Node>,attributes:&[Attribute],parent:usize) {
    for a in attributes {let id=add(nodes,Kind::Annotation,None,parent,"annotations",Some(&a.name),false);arguments(nodes,&a.args,id);}
}
fn arguments(nodes:&mut Vec<Node>,args:&[Argument],parent:usize) {
    for a in args {let id=add(nodes,Kind::Argument,None,parent,"arguments",a.name.as_deref(),false);expr(nodes,&a.value,id,"value");}
}
fn target(nodes:&mut Vec<Node>,value:&AssignmentTarget,parent:usize) {
    match value {
        AssignmentTarget::Identifier(name)=>{add(nodes,Kind::Target,None,parent,"target",Some(name),false);},
        AssignmentTarget::Member{base,member}=>{let id=add(nodes,Kind::Target,None,parent,"target",Some(member),false);expr(nodes,base,id,"base");},
        AssignmentTarget::Index{base,index}=>{let id=add(nodes,Kind::Target,None,parent,"target",None,false);expr(nodes,base,id,"base");expr(nodes,index,id,"index");},
    }
}
fn stmt(nodes:&mut Vec<Node>,value:&Statement,parent:usize,edge:&'static str)->usize {
    let (kind,name)=match &value.kind {
        StatementKind::Import{path,..}=>(Kind::Import,Some(path.as_str())),StatementKind::ClassDecl{name,..}=>(Kind::Class,Some(name.as_str())),StatementKind::InterfaceDecl{name,..}=>(Kind::Interface,Some(name.as_str())),StatementKind::FunctionDecl{name,..}=>(Kind::Function,Some(name.as_str())),StatementKind::VariableDecl{name,..}=>(Kind::Variable,Some(name.as_str())),StatementKind::Component{name,..}=>(Kind::Component,Some(name.as_str())),StatementKind::Param{name,..}=>(Kind::Param,Some(name.as_str())),
        StatementKind::ForLoop{..}=>(Kind::ForIn,None),StatementKind::ForClassic{..}=>(Kind::For,None),StatementKind::WhileLoop{..}=>(Kind::While,None),StatementKind::DoWhile{..}=>(Kind::DoWhile,None),StatementKind::If{..}=>(Kind::If,None),StatementKind::Switch{..}=>(Kind::Switch,None),StatementKind::Return(..)=>(Kind::Return,None),StatementKind::Throw(..)=>(Kind::Throw,None),StatementKind::Continue(..)=>(Kind::Continue,None),StatementKind::Break(..)=>(Kind::Break,None),StatementKind::Labeled{..}=>(Kind::Block,None),StatementKind::Rethrow=>(Kind::Rethrow,None),StatementKind::Assert{..}=>(Kind::Assert,None),StatementKind::Not(..)=>(Kind::Unary,None),StatementKind::Include(..)=>(Kind::Include,None),StatementKind::Destructure{..}=>(Kind::Destructure,None),StatementKind::BufferOutput(..)=>(Kind::Output,None),StatementKind::TryCatch{..}=>(Kind::Try,None),StatementKind::Expression(..)=>(Kind::Expression,None),
    };
    let id=add(nodes,kind,value.span,parent,edge,name,value.incomplete);
    nodes[id].body_span=value.body_span;
    match &value.kind {
        StatementKind::FunctionDecl{modifiers,..}=>{
            if let Some(a)=&modifiers.access{nodes[id].attributes.push(("access",AttributeValue::Text(a.clone())));}
            for (key,flag) in [("static",modifiers.is_static),("abstract",modifiers.is_abstract),("final",modifiers.is_final)]{nodes[id].attributes.push((key,AttributeValue::Flag(flag)));}
        },
        StatementKind::ClassDecl{extends,implements,accessors,modifiers,..}=>{
            if let Some(e)=extends{nodes[id].attributes.push(("extends",AttributeValue::Text(e.clone())));}
            nodes[id].attributes.extend([("implements",AttributeValue::Names(implements.clone())),("accessors",AttributeValue::Flag(*accessors)),("abstract",AttributeValue::Flag(modifiers.is_abstract)),("final",AttributeValue::Flag(modifiers.is_final))]);
        },
        StatementKind::Import{alias,..}=>{if let Some(a)=alias{nodes[id].attributes.push(("alias",AttributeValue::Text(a.clone())));}},
        StatementKind::ForLoop{item,index,..}=>{nodes[id].attributes.push(("item",AttributeValue::Text(item.clone())));if let Some(i)=index{nodes[id].attributes.push(("index",AttributeValue::Text(i.clone())));}},
        _=>{},
    }
    match &value.kind {
        StatementKind::FunctionDecl{params:p,body,attributes:a,return_type,..}=>{nodes[id].return_type=return_type.clone();attributes(nodes,a,id);params(nodes,p,id);function_body(nodes,body,id,value.body_span);},
        StatementKind::ClassDecl{members,attributes:a,..}=>{attributes(nodes,a,id);for member in members {match member {ClassMember::Statement(s)=>{stmt(nodes,s,id,"members");},ClassMember::Property(p)=>{let property=add(nodes,Kind::Property,Some(p.span),id,"properties",Some(&p.name),false);for a in &p.annotations {let annotation=add(nodes,Kind::Annotation,Some(a.span),property,"annotations",Some(&a.key),false);if let Some(v)=&a.value{expr(nodes,v,annotation,"value");}}}}}},
        StatementKind::InterfaceDecl{members,..}=>{block(nodes,members,id,"members",value.body_span);},
        StatementKind::VariableDecl{value,..}|StatementKind::Expression(value)|StatementKind::Not(value)|StatementKind::Include(value)|StatementKind::BufferOutput(value)=>{expr(nodes,value,id,"value");},
        StatementKind::ForLoop{collection,body,..}=>{expr(nodes,collection,id,"collection");block(nodes,body,id,"body",value.body_span);},
        StatementKind::ForClassic{init,condition,update,body}=>{if let Some(s)=init{stmt(nodes,s,id,"init");}if let Some(e)=condition{expr(nodes,e,id,"condition");}if let Some(e)=update{expr(nodes,e,id,"update");}block(nodes,body,id,"body",value.body_span);},
        StatementKind::WhileLoop{condition,body}|StatementKind::DoWhile{condition,body}=>{expr(nodes,condition,id,"condition");block(nodes,body,id,"body",value.body_span);},
        StatementKind::If{condition,then_branch,else_branch}=>{expr(nodes,condition,id,"condition");block(nodes,then_branch,id,"then",None);if let Some(body)=else_branch{block(nodes,body,id,"else",None);}},
        StatementKind::Switch{value,cases,default_case}=>{expr(nodes,value,id,"value");for case in cases{let c=add(nodes,Kind::Case,None,id,"cases",None,false);expr(nodes,&case.value,c,"value");block(nodes,&case.body,c,"body",None);}if let Some(body)=default_case{block(nodes,body,id,"default",None);}},
        StatementKind::Return(v)|StatementKind::Throw(v)=>{if let Some(v)=v{expr(nodes,v,id,"value");}},
        StatementKind::Assert{condition,message}=>{expr(nodes,condition,id,"condition");if let Some(v)=message{expr(nodes,v,id,"message");}},
        StatementKind::Param{default,..}=>{if let Some(v)=default{expr(nodes,v,id,"default");}},
        StatementKind::Destructure{source,..}=>{expr(nodes,source,id,"source");},
        StatementKind::TryCatch{try_branch,catches,finally_branch}=>{block(nodes,try_branch,id,"try",None);for catch in catches{let c=add(nodes,Kind::Catch,None,id,"catches",Some(&catch.exception_var),false);block(nodes,&catch.body,c,"body",None);}if let Some(body)=finally_branch{block(nodes,body,id,"finally",None);}},
        StatementKind::Component{attributes,body,..}=>{for (key,v) in attributes {expr(nodes,key,id,"attributeKey");expr(nodes,v,id,"attributeValue");}if let Some(body)=body{block(nodes,body,id,"body",value.body_span);}},
        StatementKind::Labeled{body,..}=>{block(nodes,body,id,"body",None);},
        StatementKind::Import{..}|StatementKind::Continue(..)|StatementKind::Break(..)|StatementKind::Rethrow=>{},
    } id
}
fn expr(nodes:&mut Vec<Node>,value:&Expression,parent:usize,edge:&'static str)->usize {
    let (kind,name)=match &value.kind {
        ExpressionKind::New{class_path,..}=>(Kind::New,Some(class_path.as_str())),ExpressionKind::Assignment{..}=>(Kind::Assignment,None),ExpressionKind::Binary{operator,..}=>(Kind::Binary,Some(operator.as_str())),ExpressionKind::UnaryNot(..)|ExpressionKind::UnaryBitwiseNot(..)=>(Kind::Unary,None),ExpressionKind::Ternary{..}=>(Kind::Ternary,None),ExpressionKind::Elvis{..}=>(Kind::Elvis,None),ExpressionKind::FunctionCall{..}=>(Kind::Call,None),ExpressionKind::ArrayAccess{..}=>(Kind::Index,None),ExpressionKind::MemberAccess{member,..}=>(Kind::Member,Some(member.as_str())),ExpressionKind::SafeMemberAccess{member,..}=>(Kind::SafeMember,Some(member.as_str())),ExpressionKind::Prefix{operator,..}=>(Kind::Prefix,Some(operator.as_str())),ExpressionKind::Postfix{operator,..}=>(Kind::Postfix,Some(operator.as_str())),ExpressionKind::Spread(..)=>(Kind::Spread,None),ExpressionKind::Identifier(name)=>(Kind::Identifier,Some(name.as_str())),
        ExpressionKind::Literal(literal)=>(match literal {Literal::String(..)=>Kind::String,Literal::Number(..)=>Kind::Number,Literal::Boolean(..)=>Kind::Boolean,Literal::Null=>Kind::Null,Literal::Array(..)=>Kind::Array,Literal::Struct(..)=>Kind::Struct,Literal::Function{is_lambda,..}=>if *is_lambda{Kind::Lambda}else{Kind::Closure}},None),
    };
    let id=add(nodes,kind,value.span,parent,edge,name,value.incomplete);
    match &value.kind {
        ExpressionKind::Literal(Literal::String(parts))=>{
            let plain=parts.iter().all(|p|matches!(p,StringPart::Text(_)));
            nodes[id].attributes.push(("interpolated",AttributeValue::Flag(!plain)));
            if plain{nodes[id].attributes.push(("value",AttributeValue::Text(parts.iter().filter_map(|p|if let StringPart::Text(t)=p{Some(t.as_str())}else{None}).collect())));}
        },
        ExpressionKind::Literal(Literal::Boolean(value))=>nodes[id].attributes.push(("value",AttributeValue::Flag(*value))),
        ExpressionKind::Literal(Literal::Number(value))=>nodes[id].attributes.push(("value",AttributeValue::Number(*value))),
        _=>{},
    }
    match &value.kind {
        ExpressionKind::New{args,..}=>arguments(nodes,args,id),ExpressionKind::Assignment{target:t,value}=>{target(nodes,t,id);expr(nodes,value,id,"value");},ExpressionKind::Binary{left,right,..}|ExpressionKind::Elvis{left,right}=>{expr(nodes,left,id,"left");expr(nodes,right,id,"right");},
        ExpressionKind::UnaryNot(v)|ExpressionKind::UnaryBitwiseNot(v)|ExpressionKind::Spread(v)=>{expr(nodes,v,id,"value");},ExpressionKind::Ternary{condition,then_expr,else_expr}=>{expr(nodes,condition,id,"condition");expr(nodes,then_expr,id,"then");expr(nodes,else_expr,id,"else");},ExpressionKind::FunctionCall{base,args}=>{expr(nodes,base,id,"base");arguments(nodes,args,id);},ExpressionKind::ArrayAccess{base,index}=>{expr(nodes,base,id,"base");expr(nodes,index,id,"index");},ExpressionKind::MemberAccess{base,..}|ExpressionKind::SafeMemberAccess{base,..}|ExpressionKind::Postfix{base,..}=>{expr(nodes,base,id,"base");},ExpressionKind::Prefix{target:t,..}=>target(nodes,t,id),
        ExpressionKind::Literal(Literal::Array(values))=>{for v in values{expr(nodes,v,id,"elements");}},ExpressionKind::Literal(Literal::Struct(values))=>{for (k,v) in values{expr(nodes,k,id,"key");expr(nodes,v,id,"value");}},ExpressionKind::Literal(Literal::String(parts))=>{for p in parts{if let StringPart::Expression(v)=p{expr(nodes,v,id,"interpolation");}}},ExpressionKind::Literal(Literal::Function{params:p,body,..})=>{params(nodes,p,id);function_body(nodes,body,id,None);},ExpressionKind::Identifier(..)|ExpressionKind::Literal(..)=>{},
    }id
}
