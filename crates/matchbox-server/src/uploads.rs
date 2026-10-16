use crate::{multipart::UploadedFile, webroot_core::WebrootRequest};
use matchbox_vm::types::{BxVM, BxValue};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
fn text(vm: &mut dyn BxVM, id: usize, key: &str, value: impl Into<String>) {
    let value = vm.string_new(value.into());
    vm.struct_set(id, key, BxValue::new_ptr(value));
}
fn list(vm: &dyn BxVM, value: BxValue) -> Vec<String> {
    if value.is_null() {
        return vec![];
    }
    if vm.is_array_value(value) {
        let id = value.as_gc_id().unwrap();
        return (0..vm.array_len(id))
            .map(|index| vm.to_string(vm.array_get(id, index)))
            .collect();
    }
    vm.to_string(value)
        .split(',')
        .map(|value| value.trim().to_string())
        .collect()
}
fn valid_name(name: &str) -> bool {
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_control() || "/\\:<>\"|?*".contains(c))
        && !name.ends_with(['.', ' '])
        && !matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !(device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'))
}
pub(crate) fn call(
    vm: &mut dyn BxVM,
    request: &WebrootRequest,
    root: &Path,
    args: &[BxValue],
    all: bool,
) -> Result<BxValue, String> {
    let arg = |index| args.get(index).copied().unwrap_or_else(BxValue::new_null);
    if arg(0).is_null() || vm.to_string(arg(0)).is_empty() {
        return Err("FileUpload requires destination".into());
    }
    if request.uploads.is_empty() {
        return Err("No file uploads were found in the request".into());
    }
    let field = if arg(1).is_null() {
        None
    } else {
        Some(vm.to_string(arg(1)))
    };
    let selected: Vec<_> = request
        .uploads
        .iter()
        .filter(|upload| {
            all || field
                .as_ref()
                .is_none_or(|name| upload.field.eq_ignore_ascii_case(name))
        })
        .take(if all { usize::MAX } else { 1 })
        .collect();
    if selected.is_empty() {
        return Err("The specified upload field was not found".into());
    }
    let destination = vm.to_string(arg(0));
    let conflict = if arg(3).is_null() {
        "error".into()
    } else {
        vm.to_string(arg(3)).to_ascii_lowercase()
    };
    if !matches!(
        conflict.as_str(),
        "error" | "skip" | "overwrite" | "makeunique"
    ) {
        return Err("Invalid upload nameConflict".into());
    }
    let strict = if arg(4).is_null() {
        true
    } else {
        vm.cast_value_to_type(arg(4), "boolean")?.as_bool()
    };
    let allowed = list(vm, arg(5));
    let blocked = list(vm, arg(6));
    let accept = list(vm, if arg(2).is_null() { arg(9) } else { arg(2) });
    if !arg(7).is_null() || !arg(8).is_null() {
        return Err("Upload attributes/mode are not supported".into());
    }
    let mut directory = PathBuf::from(&destination);
    if destination.starts_with('.') {
        directory = crate::web_context::response_file(vm, root, &destination)?;
    } else if !directory.is_absolute() {
        directory = std::env::temp_dir().join(directory)
    }
    let create = directory.starts_with(std::env::temp_dir());
    let explicit = if !directory.is_dir()
        && directory
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains('.'))
    {
        let name = directory
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        directory.pop();
        Some(name)
    } else {
        None
    };
    if create {
        std::fs::create_dir_all(&directory)
            .map_err(|e| format!("Creating upload directory: {e}"))?;
    }
    let directory = directory
        .canonicalize()
        .map_err(|e| format!("Upload directory must exist: {e}"))?;
    if !directory.is_dir() {
        return Err("Upload destination must be a directory".into());
    }
    let array = vm.array_new();
    let _array_root = vm.retain_value(BxValue::new_ptr(array))?;
    for upload in selected {
        let result = save(
            vm,
            upload,
            &directory,
            explicit.as_deref(),
            &conflict,
            strict,
            &allowed,
            &blocked,
            &accept,
        )?;
        if !all {
            return Ok(result);
        }
        vm.array_push(array, result);
    }
    Ok(BxValue::new_ptr(array))
}
#[allow(clippy::too_many_arguments)]
fn save(
    vm: &mut dyn BxVM,
    upload: &UploadedFile,
    directory: &Path,
    explicit: Option<&str>,
    conflict: &str,
    strict: bool,
    allowed: &[String],
    blocked: &[String],
    accept: &[String],
) -> Result<BxValue, String> {
    let name = explicit.unwrap_or(&upload.filename);
    if !valid_name(&upload.filename) || !valid_name(name) {
        return Err("Upload filenames must be portable basenames, not paths".into());
    }
    let extension = Path::new(&upload.filename)
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let mime = mime_guess::from_path(&upload.filename)
        .first_or_octet_stream()
        .to_string();
    let matches = |items: &[String], wildcard: bool| {
        items.iter().any(|item| {
            (wildcard && item == "*")
                || item
                    .trim_start_matches('.')
                    .eq_ignore_ascii_case(&extension)
        })
    };
    let mut permitted = (allowed.is_empty() || matches(allowed, true)) && !matches(blocked, false);
    if allowed.is_empty() && blocked.is_empty() && !accept.is_empty() {
        permitted = accept.iter().any(|item| {
            item == "*"
                || item == "*/*"
                || item.eq_ignore_ascii_case(&mime)
                || item.strip_suffix("/*").is_some_and(|prefix| {
                    mime.to_ascii_lowercase()
                        .starts_with(&(prefix.to_ascii_lowercase() + "/"))
                })
        });
    }
    let mut configured = false;
    let mut app_allowed = vec![];
    let mut app_blocked = vec![];
    if let Some(value) = vm.get_global_value("__mb_upload_allowed") {
        configured = true;
        app_allowed = list(vm, value);
    }
    if let Some(value) = vm.get_global_value("__mb_upload_blocked") {
        configured = true;
        app_blocked = list(vm, value);
    }
    if (strict || allowed.is_empty() && blocked.is_empty() && accept.is_empty()) && configured {
        permitted &= matches(&app_allowed, true) || !matches(&app_blocked, false);
    }
    let path = directory.join(name);
    let record = vm.struct_new();
    let _root = vm.retain_value(BxValue::new_ptr(record))?;
    for key in [
        "attemptedServerFile",
        "clientDirectory",
        "clientFile",
        "clientFileExt",
        "clientFileName",
        "contentSubType",
        "contentType",
        "dateLastAccessed",
        "fileSize",
        "oldFileSize",
        "serverDirectory",
        "serverFile",
        "serverFileExt",
        "serverFileName",
        "timeCreated",
        "timeLastModified",
    ] {
        vm.struct_set(record, key, BxValue::new_null());
    }
    for key in [
        "fileExisted",
        "fileWasAppended",
        "fileWasOverwritten",
        "fileWasRenamed",
        "fileWasSaved",
    ] {
        vm.struct_set(record, key, BxValue::new_bool(false));
    }
    text(vm, record, "clientDirectory", directory.to_string_lossy());
    text(vm, record, "clientFile", &upload.filename);
    text(vm, record, "clientFileExt", &extension);
    text(
        vm,
        record,
        "clientFileName",
        Path::new(&upload.filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
    );
    text(vm, record, "attemptedServerFile", path.to_string_lossy());
    let size = std::fs::metadata(upload.path())
        .map_err(|e| format!("Reading upload metadata: {e}"))?
        .len();
    vm.struct_set(record, "fileSize", BxValue::new_number(size as f64));
    if !permitted {
        std::fs::remove_file(upload.path())
            .map_err(|e| format!("Deleting rejected upload: {e}"))?;
        if strict {
            return Err("Upload rejected by extension/MIME security settings".into());
        }
        return Ok(BxValue::new_ptr(record));
    }
    let existing = match std::fs::symlink_metadata(&path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Reading destination metadata: {error}")),
    };
    let now = chrono::Utc::now();
    let created = existing
        .as_ref()
        .and_then(|metadata| metadata.created().ok())
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or(now);
    let date = vm.datetime_new(created);
    vm.struct_set(record, "timeCreated", BxValue::new_ptr(date));
    let mut target = path.clone();
    if let Some(metadata) = existing {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("Upload destination is not a regular file".into());
        }
        vm.struct_set(record, "fileExisted", BxValue::new_bool(true));
        vm.struct_set(
            record,
            "oldFileSize",
            BxValue::new_number(metadata.len() as f64),
        );
        match conflict {
            "error" => return Err("Upload destination already exists".into()),
            "skip" => return Ok(BxValue::new_ptr(record)),
            "makeunique" => {
                let stem = Path::new(name)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy();
                let ext = Path::new(name)
                    .extension()
                    .map(|v| format!(".{}", v.to_string_lossy()))
                    .unwrap_or_default();
                target = directory.join(format!("{stem}_{}{ext}", uuid::Uuid::new_v4()));
                vm.struct_set(record, "fileWasRenamed", BxValue::new_bool(true));
            }
            _ => {
                vm.struct_set(record, "fileWasOverwritten", BxValue::new_bool(true));
            }
        }
    }
    // Stage in the destination directory: failures never truncate the previous file.
    let mut staged = tempfile::NamedTempFile::new_in(directory)
        .map_err(|e| format!("Creating upload destination: {e}"))?;
    let mut source =
        std::fs::File::open(upload.path()).map_err(|e| format!("Reading upload: {e}"))?;
    std::io::copy(&mut source, &mut staged).map_err(|e| format!("Copying upload: {e}"))?;
    staged
        .flush()
        .map_err(|e| format!("Flushing upload: {e}"))?;
    if conflict == "overwrite" {
        staged
            .persist(&target)
            .map_err(|e| format!("Saving upload: {e}"))?;
    } else {
        staged
            .persist_noclobber(&target)
            .map_err(|e| format!("Saving upload without overwrite: {e}"))?;
    }
    let file = target.file_name().unwrap().to_string_lossy();
    let extension = target.extension().unwrap_or_default().to_string_lossy();
    text(vm, record, "serverFile", file);
    text(vm, record, "serverFileExt", extension);
    text(
        vm,
        record,
        "serverFileName",
        target.file_stem().unwrap_or_default().to_string_lossy(),
    );
    text(vm, record, "serverDirectory", directory.to_string_lossy());
    let mime = mime_guess::from_path(&target)
        .first_or_octet_stream()
        .to_string();
    let (major, minor) = mime.split_once('/').unwrap();
    text(vm, record, "contentType", major);
    text(vm, record, "contentSubType", minor);
    text(vm, record, "mimeType", mime.clone());
    let date = vm.datetime_new(now);
    vm.struct_set(record, "timeLastModified", BxValue::new_ptr(date));
    vm.struct_set(record, "dateLastAccessed", BxValue::new_ptr(date));
    vm.struct_set(record, "fileWasSaved", BxValue::new_bool(true));
    Ok(BxValue::new_ptr(record))
}
