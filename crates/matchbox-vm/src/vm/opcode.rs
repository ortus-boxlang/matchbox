/// Flat u32 opcode encoding.
///
/// Word 0: low 8 bits = opcode, high 24 bits = first operand (op0)
/// Multi-word instructions have additional 32-bit words for extra operands.
///
/// Instruction widths:
///   1-word: most instructions
///   2-word: COMPARE_JUMP, CALL_NAMED, INVOKE
///   3-word: LOCAL_COMPARE_JUMP, GLOBAL_COMPARE_JUMP, INVOKE_NAMED, INVOKE_NAMED_SPREAD, ITER_NEXT, LOCAL_JUMP_IF_NE_CONST, FOR_LOOP_STEP
pub mod op {
    pub const INC_LOCAL: u8 = 0;
    pub const LOCAL_COMPARE_JUMP: u8 = 1; // 3-word: op0=slot, w1=const_idx, w2=offset
    pub const COMPARE_JUMP: u8 = 2; // 2-word: op0=const_idx, w1=offset
    pub const INC_GLOBAL: u8 = 3;
    pub const GLOBAL_COMPARE_JUMP: u8 = 4; // 3-word: op0=name_idx, w1=const_idx, w2=offset
    pub const GET_LOCAL: u8 = 5;
    pub const SET_LOCAL: u8 = 6;
    pub const SET_LOCAL_POP: u8 = 7;
    pub const CONSTANT: u8 = 8;
    pub const ADD_INT: u8 = 9;
    pub const ADD_FLOAT: u8 = 10;
    pub const ADD: u8 = 11;
    pub const SUBTRACT: u8 = 12;
    pub const SUB_INT: u8 = 13;
    pub const SUB_FLOAT: u8 = 14;
    pub const MULTIPLY: u8 = 15;
    pub const MUL_INT: u8 = 16;
    pub const MUL_FLOAT: u8 = 17;
    pub const DIVIDE: u8 = 18;
    pub const DIV_FLOAT: u8 = 19;
    pub const POP: u8 = 20;
    pub const JUMP_IF_FALSE: u8 = 21;
    pub const JUMP: u8 = 22;
    pub const LOOP: u8 = 23;
    pub const RETURN: u8 = 24;
    pub const GET_GLOBAL: u8 = 25;
    pub const SET_GLOBAL: u8 = 26;
    pub const SET_GLOBAL_POP: u8 = 27;
    pub const DEFINE_GLOBAL: u8 = 28;
    pub const GET_PRIVATE: u8 = 29;
    pub const SET_PRIVATE: u8 = 30;
    pub const DUP: u8 = 31;
    pub const SWAP: u8 = 32;
    pub const OVER: u8 = 33;
    pub const INC: u8 = 34;
    pub const DEC: u8 = 35;
    pub const ARRAY: u8 = 36;
    pub const STRUCT: u8 = 37;
    pub const INDEX: u8 = 38;
    pub const SET_INDEX: u8 = 39;
    pub const MEMBER: u8 = 40;
    pub const SET_MEMBER: u8 = 41;
    pub const INC_MEMBER: u8 = 42;
    pub const STRING_CONCAT: u8 = 43;
    pub const CALL: u8 = 44;
    pub const CALL_NAMED: u8 = 45; // 2-word: op0=total_count, w1=names_idx
    pub const CALL_SPREAD: u8 = 46; // 1-word: op0=arg_entry_count
    pub const INVOKE: u8 = 47; // 2-word: op0=name_idx, w1=arg_count
    pub const INVOKE_NAMED: u8 = 48; // 3-word: op0=name_idx, w1=total_count, w2=names_idx
    pub const NEW: u8 = 49;
    pub const EQUAL: u8 = 50;
    pub const NOT_EQUAL: u8 = 51;
    pub const LESS: u8 = 52;
    pub const LESS_EQUAL: u8 = 53;
    pub const GREATER: u8 = 54;
    pub const GREATER_EQUAL: u8 = 55;
    pub const NOT: u8 = 56;
    pub const ITER_NEXT: u8 = 57; // 3-word: op0=collection_slot, w1=cursor_slot|(has_index<<31), w2=exit_offset
    pub const LOCAL_JUMP_IF_NE_CONST: u8 = 58; // 3-word: op0=slot, w1=const_idx, w2=offset
    pub const PUSH_HANDLER: u8 = 59; // 2-word: op0=absolute catch ip or 0, w1=absolute finally ip or 0
    pub const POP_HANDLER: u8 = 60;
    pub const THROW: u8 = 61;
    pub const PRINT: u8 = 62;
    pub const PRINTLN: u8 = 63;
    pub const FOR_LOOP_STEP: u8 = 64; // 3-word: op0=slot, w1=const_idx, w2=offset — increment local, compare < const, jump back if true
    pub const JUMP_IF_NULL: u8 = 65;
    pub const MODULO: u8 = 66;
    // Bitwise operators
    pub const BIT_OR: u8 = 67;
    pub const BIT_AND: u8 = 68;
    pub const BIT_XOR: u8 = 69;
    pub const BIT_NOT: u8 = 70;
    pub const BIT_SHL: u8 = 71;
    pub const BIT_SHR: u8 = 72;
    pub const BIT_USHR: u8 = 73;
    pub const POW: u8 = 74;
    pub const XOR_OP: u8 = 75;
    pub const EQV_OP: u8 = 76;
    pub const RANGE: u8 = 77;
    pub const BUFFER_WRITE: u8 = 78;
    pub const CONTAINS: u8 = 79;
    pub const ARRAY_SPREAD: u8 = 80;
    pub const STRUCT_SPREAD: u8 = 81;
    pub const INSTANCEOF: u8 = 82;
    pub const CASTAS: u8 = 83;
    pub const CALL_NAMED_SPREAD: u8 = 84; // 1-word: op0=arg_entry_count
    pub const INVOKE_NAMED_SPREAD: u8 = 85; // 3-word: op0=name_idx, w1=arg_entry_count, w2=unused
    pub const END_FINALLY: u8 = 86;
    pub const INTEGER_DIVIDE: u8 = 87;
    pub const STRICT_EQUAL: u8 = 88;
    pub const STRICT_NOT_EQUAL: u8 = 89;
    pub const SAFE_INDEX: u8 = 90;
    pub const BIND_LOCAL: u8 = 91; // 2-word: op0=slot, w1=name_idx
    pub const FUNCTION_ENTER: u8 = 92;
    pub const DUP_BELOW: u8 = 93; // op0=n: copy stack[len-1-n] to just below the top n values
    pub const CTOR_FINISH: u8 = 94; // pops init() result; keeps the instance below it unless the result is non-null
}

pub fn opcode_name(op: u8) -> &'static str {
    match op {
        op::INC_LOCAL => "INC_LOCAL",
        op::LOCAL_COMPARE_JUMP => "LOCAL_COMPARE_JUMP",
        op::COMPARE_JUMP => "COMPARE_JUMP",
        op::INC_GLOBAL => "INC_GLOBAL",
        op::GLOBAL_COMPARE_JUMP => "GLOBAL_COMPARE_JUMP",
        op::GET_LOCAL => "GET_LOCAL",
        op::SET_LOCAL => "SET_LOCAL",
        op::SET_LOCAL_POP => "SET_LOCAL_POP",
        op::CONSTANT => "CONSTANT",
        op::ADD_INT => "ADD_INT",
        op::ADD_FLOAT => "ADD_FLOAT",
        op::ADD => "ADD",
        op::SUBTRACT => "SUBTRACT",
        op::SUB_INT => "SUB_INT",
        op::SUB_FLOAT => "SUB_FLOAT",
        op::MULTIPLY => "MULTIPLY",
        op::MUL_INT => "MUL_INT",
        op::MUL_FLOAT => "MUL_FLOAT",
        op::DIVIDE => "DIVIDE",
        op::DIV_FLOAT => "DIV_FLOAT",
        op::POP => "POP",
        op::JUMP_IF_FALSE => "JUMP_IF_FALSE",
        op::JUMP => "JUMP",
        op::LOOP => "LOOP",
        op::RETURN => "RETURN",
        op::GET_GLOBAL => "GET_GLOBAL",
        op::SET_GLOBAL => "SET_GLOBAL",
        op::SET_GLOBAL_POP => "SET_GLOBAL_POP",
        op::DEFINE_GLOBAL => "DEFINE_GLOBAL",
        op::GET_PRIVATE => "GET_PRIVATE",
        op::SET_PRIVATE => "SET_PRIVATE",
        op::DUP => "DUP",
        op::SWAP => "SWAP",
        op::OVER => "OVER",
        op::INC => "INC",
        op::DEC => "DEC",
        op::ARRAY => "ARRAY",
        op::STRUCT => "STRUCT",
        op::INDEX => "INDEX",
        op::SET_INDEX => "SET_INDEX",
        op::MEMBER => "MEMBER",
        op::SET_MEMBER => "SET_MEMBER",
        op::INC_MEMBER => "INC_MEMBER",
        op::STRING_CONCAT => "STRING_CONCAT",
        op::CALL => "CALL",
        op::CALL_NAMED => "CALL_NAMED",
        op::CALL_SPREAD => "CALL_SPREAD",
        op::INVOKE => "INVOKE",
        op::INVOKE_NAMED => "INVOKE_NAMED",
        op::NEW => "NEW",
        op::EQUAL => "EQUAL",
        op::NOT_EQUAL => "NOT_EQUAL",
        op::LESS => "LESS",
        op::LESS_EQUAL => "LESS_EQUAL",
        op::GREATER => "GREATER",
        op::GREATER_EQUAL => "GREATER_EQUAL",
        op::NOT => "NOT",
        op::ITER_NEXT => "ITER_NEXT",
        op::LOCAL_JUMP_IF_NE_CONST => "LOCAL_JUMP_IF_NE_CONST",
        op::PUSH_HANDLER => "PUSH_HANDLER",
        op::POP_HANDLER => "POP_HANDLER",
        op::THROW => "THROW",
        op::PRINT => "PRINT",
        op::PRINTLN => "PRINTLN",
        op::FOR_LOOP_STEP => "FOR_LOOP_STEP",
        op::JUMP_IF_NULL => "JUMP_IF_NULL",
        op::MODULO => "MODULO",
        op::BIT_OR => "BIT_OR",
        op::BIT_AND => "BIT_AND",
        op::BIT_XOR => "BIT_XOR",
        op::BIT_NOT => "BIT_NOT",
        op::BIT_SHL => "BIT_SHL",
        op::BIT_SHR => "BIT_SHR",
        op::BIT_USHR => "BIT_USHR",
        op::POW => "POW",
        op::XOR_OP => "XOR_OP",
        op::EQV_OP => "EQV_OP",
        op::RANGE => "RANGE",
        op::BUFFER_WRITE => "BUFFER_WRITE",
        op::CONTAINS => "CONTAINS",
        op::ARRAY_SPREAD => "ARRAY_SPREAD",
        op::STRUCT_SPREAD => "STRUCT_SPREAD",
        op::INSTANCEOF => "INSTANCEOF",
        op::CASTAS => "CASTAS",
        op::CALL_NAMED_SPREAD => "CALL_NAMED_SPREAD",
        op::INVOKE_NAMED_SPREAD => "INVOKE_NAMED_SPREAD",
        op::END_FINALLY => "END_FINALLY",
        op::INTEGER_DIVIDE => "INTEGER_DIVIDE",
        op::STRICT_EQUAL => "STRICT_EQUAL",
        op::STRICT_NOT_EQUAL => "STRICT_NOT_EQUAL",
        op::SAFE_INDEX => "SAFE_INDEX",
        op::BIND_LOCAL => "BIND_LOCAL",
        op::FUNCTION_ENTER => "FUNCTION_ENTER",
        op::DUP_BELOW => "DUP_BELOW",
        op::CTOR_FINISH => "CTOR_FINISH",
        _ => "UNKNOWN",
    }
}
