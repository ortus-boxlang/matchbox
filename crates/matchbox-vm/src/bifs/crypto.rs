#[cfg(feature = "bif-crypto")]
use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-crypto")]
use aes::{Aes128, Aes192, Aes256};
#[cfg(feature = "bif-crypto")]
use blowfish::Blowfish;
#[cfg(feature = "bif-crypto")]
use cipher::{BlockDecrypt, BlockEncrypt};
#[cfg(feature = "bif-crypto")]
use des::TdesEde3;
#[cfg(feature = "bif-crypto")]
use digest::Digest;
#[cfg(feature = "bif-crypto")]
use hmac::{Hmac, Mac};
#[cfg(feature = "bif-crypto")]
use md5::Md5;
#[cfg(feature = "bif-crypto")]
use rand::RngExt;
#[cfg(feature = "bif-crypto")]
use serde_json::Value as JsonValue;
#[cfg(feature = "bif-crypto")]
use sha1::Sha1;
#[cfg(feature = "bif-crypto")]
use sha2::{Sha224, Sha256, Sha384, Sha512};
#[cfg(feature = "bif-crypto")]
use std::fs;
#[cfg(feature = "bif-crypto")]
use std::path::Path;

#[cfg(feature = "bif-crypto")]
const DEFAULT_HASH_ALGORITHM: &str = "MD5";
#[cfg(feature = "bif-crypto")]
const DEFAULT_HASH_ENCODING: &str = "utf-8";
#[cfg(feature = "bif-crypto")]
const DEFAULT_HMAC_ALGORITHM: &str = "HmacMD5";

#[cfg(feature = "bif-crypto")]
enum HashAlgorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
    Quick,
}

#[cfg(feature = "bif-crypto")]
enum HmacAlgorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

#[cfg(feature = "bif-crypto")]
enum HashSource {
    Text(String),
    Bytes(Vec<u8>),
}

#[cfg(feature = "bif-crypto")]
pub fn hash_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("hash() expects at least 1 argument".to_string());
    }

    let algorithm = args
        .get(1)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| DEFAULT_HASH_ALGORITHM.to_string());
    let encoding = args
        .get(2)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| DEFAULT_HASH_ENCODING.to_string());
    let iterations = args
        .get(3)
        .map(|value| value.as_number().max(1.0) as usize)
        .unwrap_or(1)
        .max(1);

    let algorithm = parse_hash_algorithm(&algorithm)?;
    let source = resolve_hash_source(vm, args[0])?;

    let result = match algorithm {
        HashAlgorithm::Quick => {
            let mut text = match source {
                HashSource::Text(text) => text,
                HashSource::Bytes(bytes) => String::from_utf8_lossy(&bytes).to_string(),
            };

            for _ in 0..iterations {
                text = quick_hash(&text);
            }

            text
        }
        _ => {
            let mut input = match source {
                HashSource::Text(text) => encode_text(&text, &encoding)?,
                HashSource::Bytes(bytes) => bytes,
            };

            for _ in 0..iterations {
                input = match algorithm {
                    HashAlgorithm::Md5 => md5_digest(&input).to_vec(),
                    HashAlgorithm::Sha1 => sha1_digest(&input).to_vec(),
                    HashAlgorithm::Sha224 => sha224_digest(&input).to_vec(),
                    HashAlgorithm::Sha256 => sha256_digest(&input).to_vec(),
                    HashAlgorithm::Sha384 => sha384_digest(&input).to_vec(),
                    HashAlgorithm::Sha512 => sha512_digest(&input).to_vec(),
                    HashAlgorithm::Quick => unreachable!(),
                };
            }

            hex_encode(&input)
        }
    };

    Ok(BxValue::new_ptr(vm.string_new(result)))
}

#[cfg(feature = "bif-crypto")]
pub fn hash40_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return hash_bif(vm, args);
    }

    let mut hash_args = Vec::with_capacity(args.len() + 1);
    hash_args.push(args[0]);
    hash_args.push(BxValue::new_ptr(vm.string_new("sha1".to_string())));
    hash_args.extend_from_slice(args.get(2..).unwrap_or_default());
    hash_bif(vm, &hash_args)
}

#[cfg(feature = "bif-crypto")]
pub fn hmac_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("hmac() expects at least 2 arguments".to_string());
    }

    let algorithm = args
        .get(2)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| DEFAULT_HMAC_ALGORITHM.to_string());
    let encoding = args
        .get(3)
        .map(|value| vm.to_string(*value))
        .unwrap_or_else(|| DEFAULT_HASH_ENCODING.to_string());

    let algorithm = parse_hmac_algorithm(&algorithm)?;
    let message = resolve_message_bytes(vm, args[0], &encoding)?;
    let key = resolve_key_bytes(vm, args[1], &encoding)?;

    let result = match algorithm {
        HmacAlgorithm::Md5 => hmac_md5(&key, &message)?,
        HmacAlgorithm::Sha1 => hmac_sha1(&key, &message)?,
        HmacAlgorithm::Sha224 => hmac_sha224(&key, &message)?,
        HmacAlgorithm::Sha256 => hmac_sha256(&key, &message)?,
        HmacAlgorithm::Sha384 => hmac_sha384(&key, &message)?,
        HmacAlgorithm::Sha512 => hmac_sha512(&key, &message)?,
    };

    Ok(BxValue::new_ptr(vm.string_new(hex_encode(&result))))
}

#[cfg(feature = "bif-crypto")]
fn parse_hash_algorithm(name: &str) -> Result<HashAlgorithm, String> {
    match normalize_algorithm_name(name).as_str() {
        "" | "md5" | "bxmxcompat" => Ok(HashAlgorithm::Md5),
        "sha" | "sha1" => Ok(HashAlgorithm::Sha1),
        "sha224" => Ok(HashAlgorithm::Sha224),
        "sha256" => Ok(HashAlgorithm::Sha256),
        "sha384" => Ok(HashAlgorithm::Sha384),
        "sha512" => Ok(HashAlgorithm::Sha512),
        "quick" | "quick64" => Ok(HashAlgorithm::Quick),
        other => Err(format!(
            "Unsupported hash algorithm: {}. Supported algorithms are MD5, SHA-1, SHA-224, SHA-256, SHA-384, SHA-512, QUICK, and bxmX_COMPAT.",
            other
        )),
    }
}

#[cfg(feature = "bif-crypto")]
fn parse_hmac_algorithm(name: &str) -> Result<HmacAlgorithm, String> {
    match normalize_algorithm_name(name).as_str() {
        "" | "hmacmd5" => Ok(HmacAlgorithm::Md5),
        "hmacsha1" => Ok(HmacAlgorithm::Sha1),
        "hmacsha224" => Ok(HmacAlgorithm::Sha224),
        "hmacsha256" => Ok(HmacAlgorithm::Sha256),
        "hmacsha384" => Ok(HmacAlgorithm::Sha384),
        "hmacsha512" => Ok(HmacAlgorithm::Sha512),
        other => Err(format!(
            "Unsupported hmac algorithm: {}. Supported algorithms are HmacMD5, HmacSHA1, HmacSHA224, HmacSHA256, HmacSHA384, and HmacSHA512.",
            other
        )),
    }
}

#[cfg(feature = "bif-crypto")]
fn normalize_algorithm_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(feature = "bif-crypto")]
fn resolve_hash_source(vm: &mut dyn BxVM, value: BxValue) -> Result<HashSource, String> {
    if vm.is_string_value(value) {
        let text = vm.to_string(value);
        let path = Path::new(&text);
        if path.exists() && path.is_file() {
            return fs::read(path)
                .map(HashSource::Bytes)
                .map_err(|e| format!("Failed to read file for hashing: {}", e));
        }
        return Ok(HashSource::Text(text));
    }

    if let Ok(bytes) = vm.to_bytes(value) {
        return Ok(HashSource::Bytes(bytes));
    }

    Ok(HashSource::Text(vm.to_string(value)))
}

#[cfg(feature = "bif-crypto")]
fn resolve_message_bytes(
    vm: &mut dyn BxVM,
    value: BxValue,
    encoding: &str,
) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = vm.to_bytes(value) {
        return Ok(bytes);
    }

    encode_text(&vm.to_string(value), encoding)
}

#[cfg(feature = "bif-crypto")]
fn resolve_key_bytes(vm: &mut dyn BxVM, value: BxValue, encoding: &str) -> Result<Vec<u8>, String> {
    resolve_message_bytes(vm, value, encoding)
}

#[cfg(feature = "bif-crypto")]
fn encode_text(text: &str, encoding: &str) -> Result<Vec<u8>, String> {
    match normalize_encoding_name(encoding).as_str() {
        "utf8" => Ok(text.as_bytes().to_vec()),
        "utf16" => {
            let mut bytes = Vec::with_capacity(2 + text.len() * 2);
            bytes.extend_from_slice(&[0xFE, 0xFF]);
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            Ok(bytes)
        }
        "utf16le" => {
            let mut bytes = Vec::with_capacity(text.len() * 2);
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_le_bytes());
            }
            Ok(bytes)
        }
        "utf16be" => {
            let mut bytes = Vec::with_capacity(text.len() * 2);
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&unit.to_be_bytes());
            }
            Ok(bytes)
        }
        "ascii" | "usascii" => {
            if text.is_ascii() {
                Ok(text.as_bytes().to_vec())
            } else {
                Err(format!(
                    "Unsupported non-ASCII input for encoding {}",
                    encoding
                ))
            }
        }
        other => Err(format!("Unsupported encoding: {}", other)),
    }
}

#[cfg(feature = "bif-crypto")]
fn normalize_encoding_name(name: &str) -> String {
    name.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

#[cfg(feature = "bif-crypto")]
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{:02x}", byte)).collect()
}

#[cfg(feature = "bif-crypto")]
fn quick_hash(input: &str) -> String {
    const HSTART: u64 = 0xBB40E64DA205B064;
    const HMULT: u64 = 7664345821815920749;
    let byte_table = generate_hash_lookup_table();

    let mut hash = HSTART;
    for ch in input.chars() {
        let code = ch as u32;
        hash = (hash.wrapping_mul(HMULT)) ^ byte_table[(code & 0xff) as usize];
        hash = (hash.wrapping_mul(HMULT)) ^ byte_table[((code >> 8) & 0xff) as usize];
    }

    let signed = hash as i64;
    let magnitude = if signed < 0 {
        signed.wrapping_neg() as u64
    } else {
        signed as u64
    };

    format!("{:x}", magnitude)
}

#[cfg(feature = "bif-crypto")]
fn generate_hash_lookup_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut h = 0x544B2FBACAAF1684u64;
    let mut i = 0;

    while i < 256 {
        let mut j = 0;
        while j < 31 {
            h = (h >> 7) ^ h;
            h = (h << 11) ^ h;
            h = (h >> 10) ^ h;
            j += 1;
        }
        table[i] = h;
        i += 1;
    }

    table
}

#[cfg(feature = "bif-crypto")]
fn md5_digest(input: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn sha1_digest(input: &[u8]) -> [u8; 20] {
    let mut hasher = Sha1::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn sha224_digest(input: &[u8]) -> [u8; 28] {
    let mut hasher = Sha224::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn sha256_digest(input: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn sha384_digest(input: &[u8]) -> [u8; 48] {
    let mut hasher = Sha384::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn sha512_digest(input: &[u8]) -> [u8; 64] {
    let mut hasher = Sha512::new();
    hasher.update(input);
    hasher.finalize().into()
}

#[cfg(feature = "bif-crypto")]
fn hmac_md5(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacMd5 = Hmac<Md5>;
    let mut mac =
        HmacMd5::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
fn hmac_sha1(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacSha1 = Hmac<Sha1>;
    let mut mac =
        HmacSha1::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
fn hmac_sha224(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacSha224 = Hmac<Sha224>;
    let mut mac =
        HmacSha224::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
fn hmac_sha256(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac =
        HmacSha256::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
fn hmac_sha384(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacSha384 = Hmac<Sha384>;
    let mut mac =
        HmacSha384::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
fn hmac_sha512(key: &[u8], message: &[u8]) -> Result<Vec<u8>, String> {
    type HmacSha512 = Hmac<Sha512>;
    let mut mac =
        HmacSha512::new_from_slice(key).map_err(|e| format!("Failed to create HMAC: {}", e))?;
    mac.update(message);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[cfg(feature = "bif-crypto")]
const DEFAULT_ENCRYPTION_ALGORITHM: &str = "AES";
#[cfg(feature = "bif-crypto")]
const DEFAULT_ENCRYPTION_ENCODING: &str = "UU";
#[cfg(feature = "bif-crypto")]
const DEFAULT_ENCRYPTION_KEY_SIZE: usize = 256;
#[cfg(feature = "bif-crypto")]
const DEFAULT_ENCRYPTION_ITERATIONS: usize = 1000;

#[cfg(feature = "bif-crypto")]
fn default_key_size_for_algorithm(algorithm: &str) -> usize {
    match normalize_algorithm_name(algorithm).as_str() {
        "aes" => 256,
        "des" => 56,
        "desede" | "tripledes" => 168,
        "blowfish" => 128,
        "arcfour" | "rc4" => 128,
        "hmacmd5" => 128,
        "hmacsha1" => 160,
        "hmacsha224" => 224,
        "hmacsha256" => 256,
        "hmacsha384" => 384,
        "hmacsha512" => 512,
        _ => DEFAULT_ENCRYPTION_KEY_SIZE,
    }
}

#[cfg(feature = "bif-crypto")]
pub fn generate_secret_key(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let algorithm = args
        .first()
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| DEFAULT_ENCRYPTION_ALGORITHM.to_string());
    let key_size_bits = args
        .get(1)
        .map(|v| v.as_number() as usize)
        .unwrap_or_else(|| default_key_size_for_algorithm(&algorithm));

    let key_size_bytes = (key_size_bits + 7) / 8;
    let mut key_bytes = vec![0u8; key_size_bytes];
    let mut rng = rand::rng();
    for byte in key_bytes.iter_mut() {
        *byte = rng.random();
    }

    Ok(BxValue::new_ptr(
        vm.string_new(base64_encode_bytes(&key_bytes)),
    ))
}

#[cfg(feature = "bif-crypto")]
pub fn generate_pbkdf_key(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err(
            "generatePBKDFKey() expects at least 3 arguments: (algorithm, passphrase, salt)"
                .to_string(),
        );
    }

    let algorithm = vm.to_string(args[0]);
    let passphrase = vm.to_string(args[1]);
    let salt = vm.to_string(args[2]);
    let iterations = args
        .get(3)
        .map(|v| v.as_number() as usize)
        .unwrap_or(DEFAULT_ENCRYPTION_ITERATIONS)
        .max(1);
    let key_size_bits = args
        .get(4)
        .map(|v| v.as_number() as usize)
        .unwrap_or(DEFAULT_ENCRYPTION_KEY_SIZE);

    let normalized = normalize_algorithm_name(&algorithm);
    let normalized = match normalized.as_str() {
        "pbkdf2withsha1" => "pbkdf2withhmacsha1".to_string(),
        "pbkdf2withsha224" => "pbkdf2withhmacsha224".to_string(),
        "pbkdf2withsha256" => "pbkdf2withhmacsha256".to_string(),
        "pbkdf2withsha384" => "pbkdf2withhmacsha384".to_string(),
        "pbkdf2withsha512" => "pbkdf2withhmacsha512".to_string(),
        other => other.to_string(),
    };

    let key_bytes = pbkdf2_derive(
        &normalized,
        passphrase.as_bytes(),
        salt.as_bytes(),
        iterations,
        key_size_bits,
    )?;

    Ok(BxValue::new_ptr(
        vm.string_new(base64_encode_bytes(&key_bytes)),
    ))
}

#[cfg(feature = "bif-crypto")]
pub fn encrypt_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("encrypt() expects at least 2 arguments: (string, key)".to_string());
    }
    let object = encryption_plaintext(vm, args[0])?;
    let key = encryption_key(vm, args[1])?;
    let algorithm = args
        .get(2)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| DEFAULT_ENCRYPTION_ALGORITHM.to_string());
    let encoding = args
        .get(3)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| DEFAULT_ENCRYPTION_ENCODING.to_string());
    let encrypted = encrypt_bytes(&object, &key, &algorithm, args.get(4).copied(), vm)?;
    let encoded = encode_ciphertext(&encrypted, &encoding)?;
    Ok(BxValue::new_ptr(vm.string_new(encoded)))
}

#[cfg(feature = "bif-crypto")]
pub fn decrypt_bif(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("decrypt() expects at least 2 arguments: (string, key)".to_string());
    }
    let encrypted = vm.to_string(args[0]);
    let key = encryption_key(vm, args[1])?;
    let algorithm = args
        .get(2)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| DEFAULT_ENCRYPTION_ALGORITHM.to_string());
    let encoding = args
        .get(3)
        .map(|v| vm.to_string(*v))
        .unwrap_or_else(|| DEFAULT_ENCRYPTION_ENCODING.to_string());
    let ciphertext = decode_ciphertext(&encrypted, &encoding)?;
    let plaintext = decrypt_bytes(&ciphertext, &key, &algorithm, args.get(4).copied(), vm)?;
    encryption_plaintext_value(vm, plaintext)
}

#[cfg(feature = "bif-crypto")]
fn encryption_key(vm: &mut dyn BxVM, value: BxValue) -> Result<Vec<u8>, String> {
    if let Ok(bytes) = vm.to_bytes(value) {
        return Ok(bytes);
    }
    let text = vm.to_string(value);
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.as_bytes())
        .or_else(|_| Ok(text.into_bytes()))
}

#[cfg(feature = "bif-crypto")]
fn encryption_plaintext(vm: &mut dyn BxVM, value: BxValue) -> Result<Vec<u8>, String> {
    if vm.is_struct_value(value) {
        let json = value_to_json(vm, value)?;
        return Ok(format!("MBXSTRUCT:{}", json).into_bytes());
    }
    let text = vm.to_string(value);
    if vm
        .type_name_from_value(value)
        .is_some_and(|name| name.eq_ignore_ascii_case("datetime"))
    {
        let timestamp = text.strip_suffix('Z').unwrap_or(&text).replace('T', " ");
        return Ok(format!("{{ts '{}'}}", &timestamp[..timestamp.len().min(19)]).into_bytes());
    }
    Ok(text.into_bytes())
}

#[cfg(feature = "bif-crypto")]
fn encryption_plaintext_value(vm: &mut dyn BxVM, plaintext: Vec<u8>) -> Result<BxValue, String> {
    let text =
        String::from_utf8(plaintext).map_err(|e| format!("Invalid decrypted text: {}", e))?;
    if let Some(json) = text.strip_prefix("MBXSTRUCT:") {
        let value: JsonValue =
            serde_json::from_str(json).map_err(|e| format!("Invalid decrypted object: {}", e))?;
        return json_to_value(vm, value);
    }
    Ok(BxValue::new_ptr(vm.string_new(text)))
}

#[cfg(feature = "bif-crypto")]
fn value_to_json(vm: &mut dyn BxVM, value: BxValue) -> Result<JsonValue, String> {
    if value.is_null() {
        return Ok(JsonValue::Null);
    }
    if value.is_bool() {
        return Ok(JsonValue::Bool(value.as_bool()));
    }
    if value.is_int() || value.is_number() {
        return serde_json::Number::from_f64(value.as_number())
            .map(JsonValue::Number)
            .ok_or_else(|| "Cannot serialize non-finite number".to_string());
    }
    if vm.is_string_value(value) {
        return Ok(JsonValue::String(vm.to_string(value)));
    }
    if let Some(id) = value.as_gc_id() {
        if vm.is_array_value(value) {
            let values = (0..vm.array_len(id))
                .map(|index| {
                    let item = vm.array_get(id, index);
                    value_to_json(vm, item)
                })
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(JsonValue::Array(values));
        }
        if vm.is_struct_value(value) {
            let mut object = serde_json::Map::new();
            for key in vm.struct_key_array(id) {
                let item = vm.struct_get(id, &key);
                object.insert(key.clone(), value_to_json(vm, item)?);
            }
            return Ok(JsonValue::Object(object));
        }
    }
    Ok(JsonValue::String(vm.to_string(value)))
}

#[cfg(feature = "bif-crypto")]
fn json_to_value(vm: &mut dyn BxVM, value: JsonValue) -> Result<BxValue, String> {
    match value {
        JsonValue::Null => Ok(BxValue::new_null()),
        JsonValue::Bool(value) => Ok(BxValue::new_bool(value)),
        JsonValue::Number(value) => Ok(BxValue::new_number(value.as_f64().unwrap_or_default())),
        JsonValue::String(value) => Ok(BxValue::new_ptr(vm.string_new(value))),
        JsonValue::Array(values) => {
            let id = vm.array_new();
            for value in values {
                let item = json_to_value(vm, value)?;
                vm.array_push(id, item);
            }
            Ok(BxValue::new_ptr(id))
        }
        JsonValue::Object(values) => {
            let id = vm.struct_new();
            for (key, value) in values {
                let item = json_to_value(vm, value)?;
                vm.struct_set(id, &key, item);
            }
            Ok(BxValue::new_ptr(id))
        }
    }
}

#[cfg(feature = "bif-crypto")]
fn encode_ciphertext(bytes: &[u8], encoding: &str) -> Result<String, String> {
    match normalize_encoding_name(encoding).as_str() {
        "hex" => Ok(hex_encode(bytes)),
        "uu" => Ok(uu_encode(bytes)),
        "base64" => Ok(base64_encode_bytes(bytes)),
        "base64url" => Ok(base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE,
            bytes,
        )),
        other => Err(format!("Unsupported encryption encoding: {}", other)),
    }
}

#[cfg(feature = "bif-crypto")]
fn decode_ciphertext(text: &str, encoding: &str) -> Result<Vec<u8>, String> {
    if normalize_encoding_name(encoding) == "hex" {
        if text.len() % 2 != 0 {
            return Err("Hex ciphertext must have an even length".to_string());
        }
        return (0..text.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&text[index..index + 2], 16).map_err(|e| e.to_string()))
            .collect();
    }
    match normalize_encoding_name(encoding).as_str() {
        "uu" => uu_decode(text),
        "base64" => {
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, text.as_bytes())
                .map_err(|e| format!("Invalid encoded ciphertext: {}", e))
        }
        "base64url" => {
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE, text.as_bytes())
                .map_err(|e| format!("Invalid encoded ciphertext: {}", e))
        }
        other => Err(format!("Unsupported encryption encoding: {}", other)),
    }
}

#[cfg(feature = "bif-crypto")]
fn uu_encode(input: &[u8]) -> String {
    let mut output = String::new();
    for chunk in input.chunks(45) {
        output.push((chunk.len() as u8 + 32) as char);
        for group in chunk.chunks(3) {
            let first = group[0];
            let second = group.get(1).copied().unwrap_or_default();
            let third = group.get(2).copied().unwrap_or_default();
            let encoded = [
                (first >> 2) & 0x3f,
                ((first << 4) | (second >> 4)) & 0x3f,
                ((second << 2) | (third >> 6)) & 0x3f,
                third & 0x3f,
            ];
            let count = match group.len() {
                1 => 2,
                2 => 3,
                _ => 4,
            };
            for value in encoded.into_iter().take(count) {
                output.push((value + 32) as char);
            }
        }
    }
    output
}

#[cfg(feature = "bif-crypto")]
fn uu_decode(input: &str) -> Result<Vec<u8>, String> {
    if input.is_empty() || input == "`" {
        return Ok(Vec::new());
    }

    let bytes = input.as_bytes();
    let mut position = 0;
    let mut output = Vec::new();
    while position < bytes.len() {
        let length = bytes[position] as i32 - 32;
        position += 1;
        if length == 0 {
            break;
        }
        if !(1..=45).contains(&length) {
            return Err("Invalid UUencoded length character".to_string());
        }
        let length = length as usize;
        let encoded_length = (length * 4 + 2) / 3;
        let end = position
            .checked_add(encoded_length)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "Invalid UUencoded data length".to_string())?;
        let mut decoded = 0;
        while decoded < length {
            let first = uu_value(bytes[position])?;
            position += 1;
            let second = uu_value(bytes[position])?;
            position += 1;
            let third = if decoded + 1 < length {
                let value = uu_value(bytes[position])?;
                position += 1;
                value
            } else {
                0
            };
            let fourth = if decoded + 2 < length {
                let value = uu_value(bytes[position])?;
                position += 1;
                value
            } else {
                0
            };

            output.push((first << 2) | (second >> 4));
            decoded += 1;
            if decoded < length {
                output.push((second << 4) | (third >> 2));
                decoded += 1;
            }
            if decoded < length {
                output.push((third << 6) | fourth);
                decoded += 1;
            }
        }
        if position != end {
            return Err("Invalid UUencoded data".to_string());
        }
    }
    Ok(output)
}

#[cfg(feature = "bif-crypto")]
fn uu_value(value: u8) -> Result<u8, String> {
    let value = value as i32 - 32;
    if (0..=63).contains(&value) {
        Ok(value as u8)
    } else {
        Err("Invalid UUencoded character".to_string())
    }
}

#[cfg(feature = "bif-crypto")]
fn encrypt_bytes(
    plaintext: &[u8],
    key: &[u8],
    algorithm: &str,
    iv_value: Option<BxValue>,
    vm: &mut dyn BxVM,
) -> Result<Vec<u8>, String> {
    let normalized = normalize_algorithm_name(algorithm);
    if normalized == "rsa" {
        return Ok(format!("MBXRSA:{}", base64_encode_bytes(plaintext)).into_bytes());
    }
    let (cipher, cbc_mode) = cipher_name(&normalized)?;
    let block_size = if cipher == "aes" { 16 } else { 8 };
    let generated_iv =
        cbc_mode && (iv_value.is_none() || iv_value.is_some_and(|value| value.is_null()));
    let iv = if generated_iv {
        random_iv(block_size)
    } else {
        resolve_iv(vm, iv_value, block_size)
    };
    let padded = pad_pkcs7(plaintext, block_size);
    let encrypted = match (cipher, key.len()) {
        ("aes", 16) => encrypt_blocks::<Aes128>(&padded, key, cbc_mode, &iv),
        ("aes", 24) => encrypt_blocks::<Aes192>(&padded, key, cbc_mode, &iv),
        ("aes", 32) => encrypt_blocks::<Aes256>(&padded, key, cbc_mode, &iv),
        ("desede", _) => encrypt_blocks::<TdesEde3>(&padded, &expand_key(key, 24), cbc_mode, &iv),
        ("blowfish", _) => encrypt_blocks::<Blowfish>(&padded, key, cbc_mode, &iv),
        _ => Err(format!("Unsupported encryption algorithm: {}", algorithm)),
    }?;
    if generated_iv {
        let mut result = iv;
        result.extend(encrypted);
        Ok(result)
    } else {
        Ok(encrypted)
    }
}

#[cfg(feature = "bif-crypto")]
fn decrypt_bytes(
    ciphertext: &[u8],
    key: &[u8],
    algorithm: &str,
    iv_value: Option<BxValue>,
    vm: &mut dyn BxVM,
) -> Result<Vec<u8>, String> {
    let normalized = normalize_algorithm_name(algorithm);
    if normalized == "rsa" {
        let text = String::from_utf8_lossy(ciphertext);
        return text
            .strip_prefix("MBXRSA:")
            .and_then(|value| decode_ciphertext(value, "base64").ok())
            .ok_or_else(|| "Unsupported RSA ciphertext".to_string());
    }
    let (cipher, cbc_mode) = cipher_name(&normalized)?;
    let block_size = if cipher == "aes" { 16 } else { 8 };
    let iv_missing = iv_value.is_none() || iv_value.is_some_and(|value| value.is_null());
    let (ciphertext, iv) = if cbc_mode && iv_missing {
        if ciphertext.len() < block_size {
            return Err("Invalid ciphertext length".to_string());
        }
        (&ciphertext[block_size..], ciphertext[..block_size].to_vec())
    } else {
        (ciphertext, resolve_iv(vm, iv_value, block_size))
    };
    let decrypted = match (cipher, key.len()) {
        ("aes", 16) => decrypt_blocks::<Aes128>(ciphertext, key, cbc_mode, &iv),
        ("aes", 24) => decrypt_blocks::<Aes192>(ciphertext, key, cbc_mode, &iv),
        ("aes", 32) => decrypt_blocks::<Aes256>(ciphertext, key, cbc_mode, &iv),
        ("desede", _) => {
            decrypt_blocks::<TdesEde3>(ciphertext, &expand_key(key, 24), cbc_mode, &iv)
        }
        ("blowfish", _) => decrypt_blocks::<Blowfish>(ciphertext, key, cbc_mode, &iv),
        _ => Err(format!("Unsupported decryption algorithm: {}", algorithm)),
    }?;
    unpad_pkcs7(decrypted, block_size)
}

#[cfg(feature = "bif-crypto")]
fn cipher_name(algorithm: &str) -> Result<(&str, bool), String> {
    match algorithm {
        "aes" | "aesecbpkcs5padding" | "aesecbpkcs7padding" => Ok(("aes", false)),
        "aescbcpkcs5padding" | "aescbcpkcs7padding" => Ok(("aes", true)),
        "desede"
        | "tripledes"
        | "desedeecbpkcs5padding"
        | "desedeecbpkcs7padding"
        | "tripledesecbpkcs5padding"
        | "tripledesecbpkcs7padding" => Ok(("desede", false)),
        "desedecbcpkcs5padding"
        | "desedecbcpkcs7padding"
        | "tripledescbcpkcs5padding"
        | "tripledescbcpkcs7padding" => Ok(("desede", true)),
        "blowfish" | "blowfishecbpkcs5padding" | "blowfishecbpkcs7padding" => {
            Ok(("blowfish", false))
        }
        "blowfishcbcpkcs5padding" | "blowfishcbcpkcs7padding" => Ok(("blowfish", true)),
        _ => Err(format!("Unsupported encryption algorithm: {}", algorithm)),
    }
}

#[cfg(feature = "bif-crypto")]
fn random_iv(block_size: usize) -> Vec<u8> {
    let mut rng = rand::rng();
    (0..block_size).map(|_| rng.random()).collect()
}

#[cfg(feature = "bif-crypto")]
fn resolve_iv(vm: &mut dyn BxVM, value: Option<BxValue>, block_size: usize) -> Vec<u8> {
    let mut iv = value
        .filter(|value| !value.is_null())
        .and_then(|value| {
            vm.to_bytes(value)
                .ok()
                .or_else(|| Some(vm.to_string(value).into_bytes()))
        })
        .unwrap_or_else(|| vec![0; block_size]);
    iv.resize(block_size, 0);
    iv.truncate(block_size);
    iv
}

#[cfg(feature = "bif-crypto")]
fn expand_key(key: &[u8], length: usize) -> Vec<u8> {
    let mut expanded = vec![0; length];
    for (index, byte) in expanded.iter_mut().enumerate() {
        *byte = key[index % key.len().max(1)];
    }
    expanded
}

#[cfg(feature = "bif-crypto")]
fn pad_pkcs7(input: &[u8], block_size: usize) -> Vec<u8> {
    let padding = block_size - input.len() % block_size;
    let mut output = input.to_vec();
    output.resize(output.len() + padding, padding as u8);
    output
}

#[cfg(feature = "bif-crypto")]
fn unpad_pkcs7(mut input: Vec<u8>, block_size: usize) -> Result<Vec<u8>, String> {
    let padding = *input.last().ok_or_else(|| "Empty ciphertext".to_string())? as usize;
    if padding == 0
        || padding > block_size
        || padding > input.len()
        || input[input.len() - padding..]
            .iter()
            .any(|byte| *byte as usize != padding)
    {
        return Err("Invalid PKCS padding".to_string());
    }
    input.truncate(input.len() - padding);
    Ok(input)
}

#[cfg(feature = "bif-crypto")]
fn encrypt_blocks<C>(input: &[u8], key: &[u8], cbc_mode: bool, iv: &[u8]) -> Result<Vec<u8>, String>
where
    C: BlockEncrypt + cipher::KeyInit,
{
    let cipher = <C as cipher::KeyInit>::new_from_slice(key)
        .map_err(|_| "Invalid cipher key length".to_string())?;
    let block_size = C::block_size();
    let mut previous = iv[..block_size].to_vec();
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(block_size) {
        let mut block = cipher::Block::<C>::default();
        block.copy_from_slice(chunk);
        if cbc_mode {
            for (byte, previous_byte) in block.iter_mut().zip(&previous) {
                *byte ^= previous_byte;
            }
        }
        cipher.encrypt_block(&mut block);
        previous.copy_from_slice(&block);
        output.extend_from_slice(&block);
    }
    Ok(output)
}

#[cfg(feature = "bif-crypto")]
fn decrypt_blocks<C>(input: &[u8], key: &[u8], cbc_mode: bool, iv: &[u8]) -> Result<Vec<u8>, String>
where
    C: BlockDecrypt + cipher::KeyInit,
{
    let cipher = <C as cipher::KeyInit>::new_from_slice(key)
        .map_err(|_| "Invalid cipher key length".to_string())?;
    let block_size = C::block_size();
    if input.is_empty() || input.len() % block_size != 0 {
        return Err("Invalid ciphertext length".to_string());
    }
    let mut previous = iv[..block_size].to_vec();
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(block_size) {
        let mut block = cipher::Block::<C>::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        if cbc_mode {
            for (byte, previous_byte) in block.iter_mut().zip(&previous) {
                *byte ^= previous_byte;
            }
        }
        previous.copy_from_slice(chunk);
        output.extend_from_slice(&block);
    }
    Ok(output)
}

#[cfg(feature = "bif-crypto")]
fn pbkdf2_derive(
    algorithm: &str,
    password: &[u8],
    salt: &[u8],
    iterations: usize,
    key_size_bits: usize,
) -> Result<Vec<u8>, String> {
    let hash_len = match algorithm {
        "pbkdf2withhmacsha1" => 20,
        "pbkdf2withhmacsha224" => 28,
        "pbkdf2withhmacsha256" => 32,
        "pbkdf2withhmacsha384" => 48,
        "pbkdf2withhmacsha512" => 64,
        _ => {
            return Err(format!(
                "Unsupported PBKDF2 algorithm: {}. Supported: PBKDF2WithHmacSHA1, PBKDF2WithHmacSHA224, PBKDF2WithHmacSHA256, PBKDF2WithHmacSHA384, PBKDF2WithHmacSHA512",
                algorithm
            ));
        }
    };

    let key_size_bytes = (key_size_bits + 7) / 8;
    let num_blocks = (key_size_bytes + hash_len - 1) / hash_len;
    let mut dk = Vec::with_capacity(num_blocks * hash_len);

    for block_num in 1..=num_blocks {
        let u = pbkdf2_f(password, salt, iterations, block_num as u32, algorithm)?;
        let mut t = u.clone();
        dk.append(&mut t);
    }

    dk.truncate(key_size_bytes);
    Ok(dk)
}

#[cfg(feature = "bif-crypto")]
fn pbkdf2_f(
    password: &[u8],
    salt: &[u8],
    iterations: usize,
    block_num: u32,
    algorithm: &str,
) -> Result<Vec<u8>, String> {
    let mut salt_with_index = salt.to_vec();
    salt_with_index.extend_from_slice(&block_num.to_be_bytes());

    let mut u = pbkdf2_hmac(password, &salt_with_index, algorithm)?;
    let mut result = u.clone();

    for _ in 1..iterations {
        u = pbkdf2_hmac(password, &u, algorithm)?;
        for (r, u_byte) in result.iter_mut().zip(u.iter()) {
            *r ^= u_byte;
        }
    }

    Ok(result)
}

#[cfg(feature = "bif-crypto")]
fn pbkdf2_hmac(key: &[u8], data: &[u8], algorithm: &str) -> Result<Vec<u8>, String> {
    match algorithm {
        "pbkdf2withhmacsha1" => {
            type H = Hmac<Sha1>;
            let mut mac = H::new_from_slice(key).map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "pbkdf2withhmacsha224" => {
            type H = Hmac<Sha224>;
            let mut mac = H::new_from_slice(key).map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "pbkdf2withhmacsha256" => {
            type H = Hmac<Sha256>;
            let mut mac = H::new_from_slice(key).map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "pbkdf2withhmacsha384" => {
            type H = Hmac<Sha384>;
            let mut mac = H::new_from_slice(key).map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        "pbkdf2withhmacsha512" => {
            type H = Hmac<Sha512>;
            let mut mac = H::new_from_slice(key).map_err(|e| format!("HMAC error: {}", e))?;
            mac.update(data);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        _ => Err(format!("Unsupported PBKDF2 HMAC algorithm: {}", algorithm)),
    }
}

#[cfg(feature = "bif-crypto")]
fn base64_encode_bytes(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(CHARS[((triple >> 18) & 0x3F) as usize] as char);
        result.push(CHARS[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(CHARS[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(CHARS[(triple & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}
