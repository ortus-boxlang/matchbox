//! `resourceRead`, `resourceReadBinary`, `resourceExists`, `resourceList`: the files packed with
//! the program (see `crate::resources`).

use crate::resources;
use crate::types::{BxVM, BxValue};

fn path_arg(vm: &mut dyn BxVM, args: &[BxValue], bif: &str) -> Result<String, String> {
    if args.is_empty() {
        return Err(format!("{bif}() expects a resource path"));
    }
    Ok(vm.to_string(args[0]))
}

/// `resourceRead( path )`: the resource as text (UTF-8).
pub fn resource_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let path = path_arg(vm, args, "resourceRead")?;
    let bytes = resources::read(&path)?;
    let text = String::from_utf8(bytes)
        .map_err(|_| format!("Resource {path} is not UTF-8 text; read it with resourceReadBinary()"))?;
    Ok(BxValue::new_ptr(vm.string_new(text)))
}

/// `resourceReadBinary( path )`: the resource as binary.
pub fn resource_read_binary(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let path = path_arg(vm, args, "resourceReadBinary")?;
    let bytes = resources::read(&path)?;
    Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
}

/// `resourceExists( path )`
pub fn resource_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let path = path_arg(vm, args, "resourceExists")?;
    Ok(BxValue::new_bool(resources::exists(&path)))
}

/// `resourceList( [dir] )`: the resource paths under `dir` (all of them without it), sorted.
pub fn resource_list(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let dir = if args.is_empty() { String::new() } else { vm.to_string(args[0]) };
    let paths = resources::list(&dir)?;
    let array = vm.array_new();
    for p in paths {
        let s = vm.string_new(p);
        vm.array_push(array, BxValue::new_ptr(s));
    }
    Ok(BxValue::new_ptr(array))
}
