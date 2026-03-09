use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OpCode {
    // Hot Loop / Specialized Opcodes
    OpIncLocal(u32),
    OpLocalCompareJump(u32, u32, u32),
    OpCompareJump(u32, u32),
    OpIncGlobal(u32),
    OpGlobalCompareJump(u32, u32, u32),

    // Basic Hot Opcodes
    OpGetLocal(u32),
    OpSetLocal(u32),
    OpSetLocalPop(u32),
    OpConstant(u32),
    OpAddInt,
    OpAddFloat,
    OpAdd,
    OpSubtract,
    OpSubInt,
    OpSubFloat,
    OpMultiply,
    OpMulInt,
    OpMulFloat,
    OpDivide,
    OpDivFloat,
    OpPop,
    OpJumpIfFalse(u32),
    OpJump(u32),
    OpLoop(u32),
    OpReturn,

    // Global / Scope Opcodes
    OpGetGlobal(u32),
    OpSetGlobal(u32),
    OpSetGlobalPop(u32),
    OpDefineGlobal(u32),
    OpGetPrivate(u32),
    OpSetPrivate(u32),

    // Stack Manipulation
    OpDup,
    OpSwap,
    OpOver,
    OpInc,
    OpDec,

    // Data Structures
    OpArray(u32),
    OpStruct(u32),
    OpIndex,
    OpSetIndex,
    OpMember(u32),
    OpSetMember(u32),
    OpIncMember(u32),
    OpStringConcat,

    // Calls / Invocations
    OpCall(u32),
    OpCallNamed(u32, u32),
    OpInvoke(u32, u32),
    OpInvokeNamed(u32, u32, u32),
    OpNew(u32),

    // Comparison
    OpEqual,
    OpNotEqual,
    OpLess,
    OpLessEqual,
    OpGreater,
    OpGreaterEqual,
    OpNot,

    // Control Flow / Misc
    OpIterNext(u32, u32, u32, bool),
    OpLocalJumpIfNeConst(u32, u32, u32),
    OpPushHandler(u32),
    OpPopHandler,
    OpThrow,
    OpPrint(u32),
    OpPrintln(u32),
}
