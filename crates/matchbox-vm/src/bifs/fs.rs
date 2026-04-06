#[cfg(feature = "bif-io")]
use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-io")]
use std::fs;
#[cfg(feature = "bif-io")]
use std::path::Path;

#[cfg(feature = "bif-io")]
use walkdir::WalkDir;

#[cfg(feature = "bif-io")]
pub fn directory_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryExists() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    Ok(BxValue::new_bool(path.exists() && path.is_dir()))
}

#[cfg(feature = "bif-io")]
pub fn directory_create(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryCreate() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);

    let recurse = if args.len() > 1 {
        args[1].as_bool()
    } else {
        true
    };

    if recurse {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        fs::create_dir(path).map_err(|e| e.to_string())?;
    }

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn directory_delete(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryDelete() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);

    let recurse = if args.len() > 1 {
        args[1].as_bool()
    } else {
        false
    };

    if recurse {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        fs::remove_dir(path).map_err(|e| e.to_string())?;
    }

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn directory_list(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("directoryList() expects at least 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let recurse = if args.len() > 1 {
        args[1].as_bool()
    } else {
        false
    };

    let array_id = vm.array_new();

    let walker = if recurse {
        WalkDir::new(&path_str).min_depth(1)
    } else {
        WalkDir::new(&path_str).min_depth(1).max_depth(1)
    };

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        let p = entry.path().to_string_lossy().to_string();
        let s_id = vm.string_new(p);
        vm.array_push(array_id, BxValue::new_ptr(s_id));
    }

    Ok(BxValue::new_ptr(array_id))
}

#[cfg(feature = "bif-io")]
pub fn file_exists(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileExists() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    // Use symlink_metadata to detect existence including symlinks
    Ok(BxValue::new_bool(path.symlink_metadata().is_ok()))
}

#[cfg(feature = "bif-io")]
pub fn file_delete(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileDelete() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    fs::remove_file(path_str).map_err(|e| e.to_string())?;
    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_move(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileMove() expects 2 arguments: (source, destination)".to_string());
    }
    let src = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);

    fs_extra::file::move_file(&src, &dest, &fs_extra::file::CopyOptions::new())
        .map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_copy(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileCopy() expects 2 arguments: (source, destination)".to_string());
    }
    let src = vm.to_string(args[0]);
    let dest = vm.to_string(args[1]);

    fs::copy(src, dest).map_err(|e| e.to_string())?;

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_info(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileInfo() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;

    let struct_id = vm.struct_new();

    vm.struct_set(
        struct_id,
        "size",
        BxValue::new_number(metadata.len() as f64),
    );
    vm.struct_set(
        struct_id,
        "is_directory",
        BxValue::new_bool(metadata.is_dir()),
    );
    vm.struct_set(struct_id, "is_file", BxValue::new_bool(metadata.is_file()));
    vm.struct_set(
        struct_id,
        "is_symlink",
        BxValue::new_bool(metadata.file_type().is_symlink()),
    );
    vm.struct_set(
        struct_id,
        "is_readonly",
        BxValue::new_bool(metadata.permissions().readonly()),
    );

    if metadata.file_type().is_symlink() {
        if let Ok(target) = fs::read_link(path) {
            let target_str = target.to_string_lossy().to_string();
            let s_id = vm.string_new(target_str);
            vm.struct_set(struct_id, "target", BxValue::new_ptr(s_id));
        }
    }

    Ok(BxValue::new_ptr(struct_id))
}

#[cfg(feature = "bif-io")]
pub fn file_create_symlink(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileCreateSymlink() expects 2 arguments: (link, target)".to_string());
    }
    let link = vm.to_string(args[0]);
    let target = vm.to_string(args[1]);

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    {
        let target_path = Path::new(&target);
        if target_path.is_dir() {
            std::os::windows::fs::symlink_dir(target, link).map_err(|e| e.to_string())?;
        } else {
            std::os::windows::fs::symlink_file(target, link).map_err(|e| e.to_string())?;
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        return Err("Symlinks not supported on this platform".to_string());
    }

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_set_executable(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileSetExecutable() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&path_str)
            .map_err(|e| e.to_string())?
            .permissions();
        perms.set_mode(perms.mode() | 0o111);
        fs::set_permissions(path_str, perms).map_err(|e| e.to_string())?;
    }

    #[cfg(not(unix))]
    {
        // No-op on Windows for executable bit
        let _ = path_str;
    }

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_read(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("fileRead() expects 1 argument".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let content = fs::read_to_string(path_str).map_err(|e| e.to_string())?;
    let s_id = vm.string_new(content);
    Ok(BxValue::new_ptr(s_id))
}

#[cfg(feature = "bif-io")]
pub fn file_write(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileWrite() expects 2 arguments: (path, content)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let content = vm.to_string(args[1]);
    fs::write(path_str, content).map_err(|e| e.to_string())?;
    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-io")]
pub fn file_append(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 {
        return Err("fileAppend() expects 2 arguments: (path, content)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let content = vm.to_string(args[1]);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path_str)
        .map_err(|e| e.to_string())?;
    file.write_all(content.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(BxValue::new_bool(true))
}
