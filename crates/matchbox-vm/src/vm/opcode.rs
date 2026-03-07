use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpCode {
    OpConstant(usize), // index into constant pool
    OpAdd,
    OpAddInt,
    OpAddFloat,
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
    OpDup,
    OpSwap,
    OpOver,
    OpInc,
    OpDec,
    OpDefineGlobal(usize), // index of name in constant pool
    OpGetGlobal(usize),    // index of name in constant pool
    OpSetGlobal(usize),    // index of name in constant pool
    OpSetGlobalPop(usize), // index of name in constant pool, pops value
    OpGetLocal(usize),     // index on the stack
    OpSetLocal(usize),     // index on the stack
    OpSetLocalPop(usize),  // index on the stack, pops value
    OpArray(usize),        // element count
    OpStruct(usize),       // pair count
    OpIndex,               // bracket access [idx]
    OpSetIndex,            // bracket assignment [idx] = val
    OpMember(usize),       // dot access .member (index in constants)
    OpSetMember(usize),    // dot assignment .member = val
    OpIncMember(usize),    // dot increment .member++ (fused)
    OpInvoke(usize, usize), // name index, arg count
    OpInvokeNamed(usize, usize, usize), // name index, total arg count, names index (in constants)
    OpCall(usize),         // arg count
    OpCallNamed(usize, usize), // total arg count, names index (in constants)
    OpIterNext(usize, usize, usize, bool), // collection slot, cursor slot, offset if done, bool if should push index
    OpNew(usize),          // arg count
    OpGetPrivate(usize),   // name index (variables scope)
    OpSetPrivate(usize),   // name index (variables scope)
    OpPushHandler(usize),  // offset to catch block
    OpPopHandler,          // remove catch handler
    OpThrow,               // throw value on stack
    OpJump(usize),         // offset to jump forward
    OpJumpIfFalse(usize),  // offset to jump forward if top of stack is falsey
    OpLocalJumpIfNeConst(usize, usize, usize), // local slot, constant index, forward offset
    OpLoop(usize),         // offset to jump backward
    OpIncGlobal(usize),    // index in global_values (via IC)
    OpIncLocal(usize) ,    // index on the stack
    OpCompareJump(usize, usize), // constant index (limit), jump offset if less than
    OpGlobalCompareJump(usize, usize, usize), // name index, limit index, jump offset
    OpLocalCompareJump(usize, usize, usize),  // local slot, limit index, jump offset
    OpReturn,
}
