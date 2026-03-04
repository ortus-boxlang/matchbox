use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpCode {
    OpConstant(usize), // index into constant pool
    OpAdd,
    OpSubtract,
    OpMultiply,
    OpDivide,
    OpStringConcat,
    OpEqual,
    OpNotEqual,
    OpLess,
    OpLessEqual,
    OpGreater,
    OpGreaterEqual,
    OpPrint(usize),        // arg count
    OpPrintln(usize),      // arg count
    OpPop,
    OpDefineGlobal(usize), // index of name in constant pool
    OpGetGlobal(usize),    // index of name in constant pool
    OpSetGlobal(usize),    // index of name in constant pool
    OpGetLocal(usize),     // index on the stack
    OpSetLocal(usize),     // index on the stack
    OpArray(usize),        // element count
    OpStruct(usize),       // pair count
    OpIndex,               // bracket access [idx]
    OpSetIndex,            // bracket assignment [idx] = val
    OpMember(usize),       // dot access .member (index in constants)
    OpSetMember(usize),    // dot assignment .member = val
    OpInvoke(usize, usize), // name index, arg count
    OpCall(usize),         // arg count
    OpIterNext(usize, usize, usize, bool), // collection slot, cursor slot, offset if done, bool if should push index
    OpNew(usize),          // arg count
    OpGetPrivate(usize),   // name index (variables scope)
    OpSetPrivate(usize),   // name index (variables scope)
    OpPushHandler(usize),  // offset to catch block
    OpPopHandler,          // remove catch handler
    OpThrow,               // throw value on stack
    OpJump(usize),         // offset to jump forward
    OpJumpIfFalse(usize),  // offset to jump forward if top of stack is falsey
    OpLoop(usize),         // offset to jump backward
    OpReturn,
}
