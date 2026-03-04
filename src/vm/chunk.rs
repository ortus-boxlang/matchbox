use crate::types::BxValue;
use super::opcode::OpCode;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Chunk {
    pub code: Vec<OpCode>,
    pub constants: Vec<BxValue>,
    pub lines: Vec<usize>,
    pub filename: String,
    #[serde(skip)]
    pub caches: Vec<Option<IcEntry>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum IcEntry {
    Monomorphic {
        shape_id: usize,
        index: usize,
    },
    // We can add Polymorphic here later
}

impl Chunk {
    pub fn new(filename: &str) -> Self {
        Chunk {
            code: Vec::new(),
            constants: Vec::new(),
            lines: Vec::new(),
            filename: filename.to_string(),
            caches: Vec::new(),
        }
    }

    pub fn write(&mut self, opcode: OpCode, line: usize) {
        self.code.push(opcode);
        self.lines.push(line);
        self.caches.push(None);
    }

    pub fn add_constant(&mut self, value: BxValue) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }
}
