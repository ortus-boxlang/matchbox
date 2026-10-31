use crate::types::{BxNativeFunction, BxVM, BxValue};
use std::collections::HashMap;

pub fn register_binary_bifs(bifs: &mut HashMap<String, BxNativeFunction>) {
    bifs.insert(
        "binarydecode".to_string(),
        binary_decode as BxNativeFunction,
    );
    bifs.insert(
        "binaryencode".to_string(),
        binary_encode as BxNativeFunction,
    );
    bifs.insert("bitand".to_string(), bit_and as BxNativeFunction);
    bifs.insert("bitor".to_string(), bit_or as BxNativeFunction);
    bifs.insert("bitxor".to_string(), bit_xor as BxNativeFunction);
    bifs.insert("bitnot".to_string(), bit_not as BxNativeFunction);
    bifs.insert(
        "bitmaskclear".to_string(),
        bit_mask_clear as BxNativeFunction,
    );
    bifs.insert("bitmaskread".to_string(), bit_mask_read as BxNativeFunction);
    bifs.insert("bitmaskset".to_string(), bit_mask_set as BxNativeFunction);
    bifs.insert("bitsh".to_string(), bit_sh as BxNativeFunction);
    bifs.insert("bitshln".to_string(), bit_sh_left as BxNativeFunction);
    bifs.insert("bitshrn".to_string(), bit_sh_right as BxNativeFunction);
}

fn binary_decode(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("binaryDecode() requires 2 arguments: string, encoding".to_string());
    }
    let input = vm.to_string(args[0]);
    let encoding = vm.to_string(args[1]).to_ascii_lowercase();
    match encoding.as_str() {
        "hex" => {
            let hex = input.trim();
            if hex.len() % 2 != 0 {
                return Err("binaryDecode() hex string must have even length".to_string());
            }
            let mut bytes = Vec::with_capacity(hex.len() / 2);
            for i in (0..hex.len()).step_by(2) {
                let byte = u8::from_str_radix(&hex[i..i + 2], 16)
                    .map_err(|e| format!("binaryDecode() invalid hex: {}", e))?;
                bytes.push(byte);
            }
            Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
        }
        "base64" => {
            let bytes = base64_decode(&input)?;
            Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
        }
        _ => Err(format!("binaryDecode() unsupported encoding: {}", encoding)),
    }
}

fn binary_encode(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("binaryEncode() requires 2 arguments: binary, encoding".to_string());
    }
    let data = vm.to_bytes(args[0])?;
    let encoding = vm.to_string(args[1]).to_ascii_lowercase();
    match encoding.as_str() {
        "hex" => {
            let hex: String = data.iter().map(|b| format!("{:02x}", b)).collect();
            Ok(BxValue::new_ptr(vm.string_new(hex)))
        }
        "base64" => {
            let encoded = base64_encode(&data);
            Ok(BxValue::new_ptr(vm.string_new(encoded)))
        }
        _ => Err(format!("binaryEncode() unsupported encoding: {}", encoding)),
    }
}

fn bit_and(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("bitAnd() requires 2 arguments".to_string());
    }
    let a = args[0].as_number() as i64;
    let b = args[1].as_number() as i64;
    Ok(BxValue::new_number((a & b) as f64))
}

fn bit_or(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("bitOr() requires 2 arguments".to_string());
    }
    let a = args[0].as_number() as i64;
    let b = args[1].as_number() as i64;
    Ok(BxValue::new_number((a | b) as f64))
}

fn bit_xor(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("bitXor() requires 2 arguments".to_string());
    }
    let a = args[0].as_number() as i64;
    let b = args[1].as_number() as i64;
    Ok(BxValue::new_number((a ^ b) as f64))
}

fn bit_not(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("bitNot() requires 1 argument".to_string());
    }
    let n = args[0].as_number() as i64;
    Ok(BxValue::new_number((!n) as f64))
}

fn bit_mask_clear(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("bitMaskClear() requires 3 arguments: number, start, length".to_string());
    }
    let number = args[0].as_number() as i64;
    let start = args[1].as_number() as i64;
    let length = args[2].as_number() as i64;
    validate_mask_range(start, length)?;
    let mask = ((1_i64 << length) - 1) << start;
    Ok(BxValue::new_number((number & !mask) as f64))
}

fn bit_mask_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("bitMaskRead() requires 3 arguments: number, start, length".to_string());
    }
    let number = args[0].as_number() as i64;
    let start = args[1].as_number() as i64;
    let length = args[2].as_number() as i64;
    validate_mask_range(start, length)?;
    let mask = (1_i64 << length) - 1;
    let result = (number >> start) & mask;
    Ok(BxValue::new_number(result as f64))
}

fn bit_mask_set(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 4 {
        return Err("bitMaskSet() requires 4 arguments: number, mask, start, length".to_string());
    }
    let number = args[0].as_number() as i64;
    let mask = args[1].as_number() as i64;
    let start = args[2].as_number() as i64;
    let length = args[3].as_number() as i64;
    validate_mask_range(start, length)?;
    let bitmask = ((1_i64 << length) - 1) << start;
    let adjusted_mask = (mask & ((1_i64 << length) - 1)) << start;
    let result = (number & !bitmask) | adjusted_mask;
    Ok(BxValue::new_number(result as f64))
}

fn bit_sh(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("bitSh() requires 2 arguments: number, count".to_string());
    }
    let number = args[0].as_number() as i64;
    let count = args[1].as_number() as i32;
    let result = if count >= 0 {
        (number as u32).wrapping_shl(count as u32) as i32
    } else {
        (number as u32).wrapping_shr((-count) as u32) as i32
    };
    Ok(BxValue::new_number(result as f64))
}

fn bit_sh_left(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    bit_sh(vm, args)
}

fn bit_sh_right(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("bitShrn() requires 2 arguments: number, count".to_string());
    }
    let shifted_args = [args[0], BxValue::new_number(-args[1].as_number())];
    bit_sh(vm, &shifted_args)
}

fn validate_mask_range(start: i64, length: i64) -> Result<(), String> {
    if !(0..=31).contains(&start) {
        return Err("Start must be in the range 0-31, inclusive.".to_string());
    }
    if !(0..=31).contains(&length) {
        return Err("Length must be in the range 0-31, inclusive.".to_string());
    }
    Ok(())
}

const BASE64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut result = String::new();
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() {
            data[i + 1] as u32
        } else {
            0
        };
        let b2 = if i + 2 < data.len() {
            data[i + 2] as u32
        } else {
            0
        };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(BASE64_CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(BASE64_CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if i + 1 < data.len() {
            result.push(BASE64_CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if i + 2 < data.len() {
            result.push(BASE64_CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        i += 3;
    }
    result
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let input: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    let unpadded = input.trim_end_matches('=');
    let padding = (4 - unpadded.len() % 4) % 4;
    let input = format!("{}{}", unpadded, "=".repeat(padding));
    if input.len() % 4 != 0 {
        return Err("binaryDecode() invalid base64 length".to_string());
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let a = base64_char_val(input.as_bytes()[i])?;
        let b = base64_char_val(input.as_bytes()[i + 1])?;
        let c_val = if input.as_bytes()[i + 2] == b'=' {
            0
        } else {
            base64_char_val(input.as_bytes()[i + 2])?
        };
        let d_val = if input.as_bytes()[i + 3] == b'=' {
            0
        } else {
            base64_char_val(input.as_bytes()[i + 3])?
        };
        let triple = (a << 18) | (b << 12) | (c_val << 6) | d_val;
        result.push(((triple >> 16) & 0xFF) as u8);
        if input.as_bytes()[i + 2] != b'=' {
            result.push(((triple >> 8) & 0xFF) as u8);
        }
        if input.as_bytes()[i + 3] != b'=' {
            result.push((triple & 0xFF) as u8);
        }
        i += 4;
    }
    Ok(result)
}

fn base64_char_val(c: u8) -> Result<u32, String> {
    match c {
        b'A'..=b'Z' => Ok((c - b'A') as u32),
        b'a'..=b'z' => Ok((c - b'a' + 26) as u32),
        b'0'..=b'9' => Ok((c - b'0' + 52) as u32),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!(
            "binaryDecode() invalid base64 character: {}",
            c as char
        )),
    }
}
