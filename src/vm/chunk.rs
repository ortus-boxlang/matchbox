use crate::types::BxValue;
use super::opcode::OpCode;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<BxValue>,
    // In a real VM, we'd want line numbers here for debugging
}

impl Chunk {
    pub fn new() -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
        }
    }

    pub fn write(&mut self, opcode: OpCode) {
        self.code.push(opcode);
    }

    pub fn add_constant(&mut self, value: BxValue) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }
}
