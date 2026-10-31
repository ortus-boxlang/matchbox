//! Native syntax views; callbacks and all semantic analysis remain BoxLang.
use crate::types::{BxNativeObject,BxVM,BxValue};
use matchbox_parser::visit::Snapshot;
use std::{cell::RefCell,rc::Rc};
const BRAND:&str="matchbox.ast.snapshot.node.v1";
#[derive(Debug)]
struct View {snapshot:Rc<Snapshot>,node:usize,allocation_bytes:usize}
fn view(vm:&mut dyn BxVM,snapshot:Rc<Snapshot>,node:usize)->BxValue {
    BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(View{snapshot,node,allocation_bytes:0}))))
}
pub(super) fn parse(vm:&mut dyn BxVM,source:String,filename:Option<&str>,recover:bool,tokens:bool)->Result<BxValue,String> {
    let snapshot=Rc::new(Snapshot::parse(source,filename,recover,tokens).map_err(|e|e.to_string())?);
    // Charge owner allocation once, not once per shared node view. Native parses
    // must create GC pressure even when they allocate no boxed AST structures.
    // ponytail: pressure estimate, not exact Rust heap accounting; include deeper
    // parser capacities if byte-accurate heap budgets become a requirement.
    let allocation_bytes=snapshot.source.capacity()
        +snapshot.nodes.capacity()*std::mem::size_of::<matchbox_parser::visit::Node>()
        +snapshot.statements.capacity()*std::mem::size_of::<matchbox_parser::ast::Statement>()
        +snapshot.nodes.iter().map(|n|n.children.capacity()*std::mem::size_of::<usize>()
            +n.name.as_ref().map_or(0,String::capacity)+n.return_type.as_ref().map_or(0,String::capacity)).sum::<usize>();
    Ok(BxValue::new_ptr(vm.native_object_new(Rc::new(RefCell::new(View{snapshot,node:0,allocation_bytes})))))
}
impl BxNativeObject for View {
    fn native_data(&self,kind:&str)->Option<*mut std::ffi::c_void>{(kind==BRAND).then_some(self as *const Self as *mut std::ffi::c_void)}
    fn is_readonly(&self)->bool{true}
    fn external_allocation_bytes(&self)->usize{self.allocation_bytes}
    fn readonly_jit_property(&self,name:&str)->Option<BxValue>{
        let n=&self.snapshot.nodes[self.node];
        Some(match name {
            "kind"=>BxValue::new_number(n.kind as u16 as f64),"id"=>BxValue::new_number((self.node+1)as f64),
            "parentid"=>BxValue::new_number(if self.node==0{0.0}else{(n.parent+1)as f64}),
            "bodyid"=>BxValue::new_number(n.body.map_or(0,|id|id+1)as f64),"childcount"=>BxValue::new_number(n.children.len()as f64),
            "start"=>n.span.map_or(BxValue::new_null(),|s|BxValue::new_number(s.start as f64)),"end"=>n.span.map_or(BxValue::new_null(),|s|BxValue::new_number(s.end as f64)),
            "bodystart"=>n.body_span.map_or(BxValue::new_null(),|s|BxValue::new_number(s.start as f64)),
            "bodyend"=>n.body_span.map_or(BxValue::new_null(),|s|BxValue::new_number(s.end as f64)),
            "incomplete"=>BxValue::new_bool(n.incomplete),_=>return None,
        })
    }
    fn get_property(&self,name:&str)->BxValue{self.readonly_jit_property(&name.to_ascii_lowercase()).unwrap_or_else(BxValue::new_null)}
    fn set_property(&mut self,_name:&str,_value:BxValue){}
    fn has_method(&self,name:&str)->bool{matches!(name.to_ascii_lowercase().as_str(),"typename"|"text"|"name"|"edge"|"returntype"|"kinds"|"nodes"|"node"|"tostruct"|"position"|"children"|"child"|"attribute"|"lex")}
    fn call_method(&mut self,vm:&mut dyn BxVM,_id:usize,name:&str,args:&[BxValue])->Result<BxValue,String>{
        let n=&self.snapshot.nodes[self.node];
        match name.to_ascii_lowercase().as_str(){
            "typename"|"text"|"name"|"edge"|"returntype" if args.is_empty()=>{
                let s=match name.to_ascii_lowercase().as_str(){"typename"=>Some(n.kind.name()),"text"=>n.span.map(|s|&self.snapshot.source[s.start..s.end]),"name"=>n.name.as_deref(),"returntype"=>n.return_type.as_deref(),_=>Some(n.edge)};
                Ok(s.map_or_else(BxValue::new_null,|s|BxValue::new_ptr(vm.string_new(s.to_owned()))))
            },
            "attribute" if args.len()==1=>{
                use matchbox_parser::visit::AttributeValue;
                let key=vm.to_string(args[0]);
                Ok(match n.attributes.iter().find(|(k,_)|k.eq_ignore_ascii_case(&key)).map(|(_,v)|v){
                    Some(AttributeValue::Text(s))=>BxValue::new_ptr(vm.string_new(s.clone())),
                    Some(AttributeValue::Flag(v))=>BxValue::new_bool(*v),
                    Some(AttributeValue::Number(v))=>BxValue::new_number(*v),
                    Some(AttributeValue::Names(names))=>{let out=vm.array_new();for name in names{let value=BxValue::new_ptr(vm.string_new(name.clone()));vm.array_push(out,value);}BxValue::new_ptr(out)},
                    None=>BxValue::new_null(),
                })
            },
            "children" if args.len()<=1=>{
                let edge=args.first().map(|v|vm.to_string(*v));let out=vm.array_new();
                for id in &n.children{if edge.as_ref().is_none_or(|e|self.snapshot.nodes[*id].edge.eq_ignore_ascii_case(e)){let child=view(vm,Rc::clone(&self.snapshot),*id);vm.array_push(out,child);}}
                Ok(BxValue::new_ptr(out))
            },
            "child" if args.len()==1=>{
                let edge=vm.to_string(args[0]);
                Ok(n.children.iter().find(|id|self.snapshot.nodes[**id].edge.eq_ignore_ascii_case(&edge)).map_or_else(BxValue::new_null,|id|view(vm,Rc::clone(&self.snapshot),*id)))
            },
            "lex" if args.is_empty()=>{
                let data=matchbox_parser::inspect::lexical_parsed(&self.snapshot.source,&self.snapshot.issues).map_err(|e|e.to_string())?;
                Ok(super::json::json_to_bx(vm,data))
            },
            "kinds" if args.is_empty()=>{
                let out=vm.struct_new();for kind in matchbox_parser::visit::Kind::ALL {vm.struct_set(out,kind.name(),BxValue::new_number(*kind as u16 as f64));}Ok(BxValue::new_ptr(out))
            },
            "node" if args.len()==1=>{
                if !args[0].is_number(){return Err("AST node id must be an integer".into());}
                let id=args[0].as_number();if !id.is_finite()||id<1.0||id.fract()!=0.0||id>self.snapshot.nodes.len()as f64{return Err("AST node id out of range".into());}
                Ok(view(vm,Rc::clone(&self.snapshot),id as usize-1))
            },
            "nodes" if args.len()<=1=>{
                let filter=args.first().map(|v|vm.to_string(*v));let array=vm.array_new();
                for (id,n) in self.snapshot.nodes.iter().enumerate(){if filter.as_ref().is_none_or(|kind|n.kind.name().eq_ignore_ascii_case(kind)){let v=view(vm,Rc::clone(&self.snapshot),id);vm.array_push(array,v);}}
                Ok(BxValue::new_ptr(array))
            },
            "tostruct" if args.is_empty()=>{
                if self.node!=0{return Err("toStruct() requires the snapshot root".into());}
                let s=&self.snapshot;let data=matchbox_parser::inspect::inspect_parsed(&s.source,&s.statements,&s.issues,s.recover,s.include_tokens).map_err(|e|e.to_string())?;
                Ok(super::json::json_to_bx(vm,data))
            },
            "position" if args.is_empty()=>{
                // ponytail: rescans source prefixes; cache line offsets if position() becomes hot.
                let Some(span)=n.span else{return Ok(BxValue::new_null());};
                let point=|offset:usize|{let before=&self.snapshot.source[..offset];let line=before.bytes().filter(|b|*b==b'\n').count()+1;let start=before.rfind('\n').map_or(0,|i|i+1);serde_json::json!({"line":line,"column":self.snapshot.source[start..offset].encode_utf16().count()})};
                Ok(super::json::json_to_bx(vm,serde_json::json!({"start":point(span.start),"end":point(span.end)})))
            },
            _=>Err(format!("Unsupported AST view method/arguments: {name}")),
        }
    }
}
pub fn visit(vm:&mut dyn BxVM,args:&[BxValue])->Result<BxValue,String>{
    let [tree,visitor,state]=args else{return Err("boxASTVisit() requires tree, visitor, state".into());};
    let pointer=vm.native_object_data(*tree,BRAND).ok_or("boxASTVisit() requires a native AST snapshot/node")?;
    // The internal brand is supplied only by this VM-owned view, never BoxLang.
    // Clone ownership before allocations/callbacks; keep no heap/object borrow.
    let (snapshot,root)=unsafe{let v=&*(pointer as *const View);(Rc::clone(&v.snapshot),v.node)};
    let chunk=vm.current_chunk().ok_or("AST visitor requires an execution context")?;
    let method=vm.has_method_by_value(*visitor,"visit");
    for v in args{vm.push_root(*v);}
    let result=(||{
        let mut work=vec![(root,0)];let mut stopped=None;
        while let Some((id,phase))=work.pop(){
            let node=view(vm,Rc::clone(&snapshot),id);vm.push_root(node);
            let values=vec![node,BxValue::new_number(phase as f64),*state];
            let result=if method{vm.call_method_by_value(*visitor,"visit",values,Rc::clone(&chunk))}else{vm.call_function_by_value(visitor,values,Rc::clone(&chunk))};
            vm.pop_root();let result=result?;
            if !result.is_number()||!matches!(result.as_number(),0.0|1.0|2.0){return Err("AST visitor must return 0 (continue), 1 (skip), or 2 (stop)".into());}
            if result.as_number()==2.0{stopped=Some(node);break;}
            if phase==0 {
                work.push((id,1));
                if result.as_number()==0.0{for child in snapshot.nodes[id].children.iter().rev(){work.push((*child,0));}}
            }
        }
        let out=vm.struct_new();vm.struct_set(out,"state",*state);vm.struct_set(out,"completed",BxValue::new_bool(stopped.is_none()));vm.struct_set(out,"stoppedAt",stopped.unwrap_or_else(BxValue::new_null));Ok(BxValue::new_ptr(out))
    })();
    for _ in args{vm.pop_root();}result
}
