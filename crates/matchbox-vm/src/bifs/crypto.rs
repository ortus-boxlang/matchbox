#[cfg(feature = "bif-crypto")]
use crate::types::{BxValue, BxVM};
#[cfg(feature = "bif-crypto")]
use std::fs::File;
#[cfg(feature = "bif-crypto")]
use std::io::Read;

#[cfg(feature = "bif-crypto")]
use sha2::{Sha256, Digest};

#[cfg(feature = "bif-crypto")]
pub fn hash_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() { return Err("hash() expects at least 1 argument".to_string()); }
    
    let input = vm.to_string(args[0]);
    let algorithm = if args.len() > 1 { vm.to_string(args[1]).to_uppercase() } else { "SHA-256".to_string() };

    if algorithm != "SHA-256" {
        return Err(format!("Unsupported hash algorithm: {}. Only SHA-256 is supported currently.", algorithm));
    }

    let mut hasher = Sha256::new();
    
    let path = std::path::Path::new(&input);
    if path.exists() && path.is_file() {
        let mut file = File::open(path).map_err(|e| format!("Failed to open file for hashing: {}", e))?;
        let mut buffer = [0; 4096];
        loop {
            let count = file.read(&mut buffer).map_err(|e| format!("Failed to read file for hashing: {}", e))?;
            if count == 0 { break; }
            hasher.update(&buffer[..count]);
        }
    } else {
        hasher.update(input.as_bytes());
    }

    let result = hasher.finalize();
    let hex_result = format!("{:x}", result);
    
    let s_id = vm.string_new(hex_result);
    Ok(BxValue::new_ptr(s_id))
}
