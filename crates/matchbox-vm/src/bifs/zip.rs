#[cfg(feature = "bif-zip")]
use crate::types::{BxVM, BxValue};
#[cfg(feature = "bif-zip")]
use std::fs;
#[cfg(feature = "bif-zip")]
use std::io::Read;
#[cfg(feature = "bif-zip")]
use std::io::Write;
#[cfg(feature = "bif-zip")]
use std::path::Path;

#[cfg(feature = "bif-zip")]
pub fn zip_extract(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("extract() expects format, source, and destination".to_string());
    }
    let format = args
        .first()
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .unwrap_or_default();
    let zip_file_str = vm.to_string(args[1]);
    let dest_dir_str = vm.to_string(args[2]);

    if !format.is_empty() {
        if !is_zip_format(&format) {
            extract_tar_like(&zip_file_str, &dest_dir_str, &format)?;
            return Ok(BxValue::new_bool(true));
        }
    } else if !is_zip_format(&zip_file_str) {
        extract_tar_like(&zip_file_str, &dest_dir_str, "")?;
        return Ok(BxValue::new_bool(true));
    }

    let file =
        fs::File::open(&zip_file_str).map_err(|e| format!("Failed to open zip file: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("Failed to read file from zip: {}", e))?;
        let outpath = match file.enclosed_name() {
            Some(path) => Path::new(&dest_dir_str).join(path),
            None => continue,
        };

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p)
                        .map_err(|e| format!("Failed to create directory: {}", e))?;
                }
            }
            let mut outfile =
                fs::File::create(&outpath).map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut file, &mut outfile)
                .map_err(|e| format!("Failed to copy file: {}", e))?;
        }

        // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(mode) = file.unix_mode() {
                fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))
                    .map_err(|e| format!("Failed to set permissions: {}", e))?;
            }
        }
    }

    Ok(BxValue::new_bool(true))
}

#[cfg(feature = "bif-zip")]
pub fn zip_compress(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 3 {
        return Err("compress() expects format, source, and destination".to_string());
    }
    let format = args
        .first()
        .filter(|value| !value.is_null())
        .map(|value| vm.to_string(*value))
        .unwrap_or_default();
    let source_str = vm.to_string(args[1]);
    let dest_str = vm.to_string(args[2]);
    let include_base_folder = args.get(3).map(|value| value.as_bool()).unwrap_or(true);
    let overwrite = args.get(4).map(|value| value.as_bool()).unwrap_or(false);

    let source_path = Path::new(&source_str);
    if !source_path.exists() {
        return Err(format!("Source path does not exist: {}", source_str));
    }

    let dest_path = Path::new(&dest_str);
    if dest_path.exists() && !overwrite {
        return Err(format!("Destination already exists: {}", dest_str));
    }

    let format = if format.is_empty() {
        archive_format_from_path(&dest_str)
    } else {
        format.to_ascii_lowercase()
    };

    if !is_zip_format(&format) {
        if is_raw_bzip_format(&format) {
            let data =
                fs::read(source_path).map_err(|e| format!("Failed to read source file: {}", e))?;
            let compressed = compress_bzip_bytes(data)?;
            fs::write(dest_path, compressed)
                .map_err(|e| format!("Failed to create archive: {}", e))?;
            return Ok(BxValue::new_ptr(vm.string_new(dest_str)));
        }
        let bytes = build_tar_bytes(source_path, include_base_folder)?;
        let compressed = compress_tar_bytes(bytes, &format)?;
        fs::write(dest_path, compressed).map_err(|e| format!("Failed to create archive: {}", e))?;
        return Ok(BxValue::new_ptr(vm.string_new(dest_str)));
    }

    let file =
        fs::File::create(&dest_path).map_err(|e| format!("Failed to create zip file: {}", e))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    if source_path.is_file() {
        let name = source_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        zip.start_file(name, options)
            .map_err(|e| format!("Failed to start file in zip: {}", e))?;
        let data =
            fs::read(source_path).map_err(|e| format!("Failed to read source file: {}", e))?;
        zip.write_all(&data)
            .map_err(|e| format!("Failed to write to zip: {}", e))?;
    } else if source_path.is_dir() {
        for entry in walkdir::WalkDir::new(source_path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.is_file() {
                let base = if include_base_folder {
                    source_path.parent().unwrap_or(source_path)
                } else {
                    source_path
                };
                let rel = path
                    .strip_prefix(base)
                    .map_err(|e| format!("Path error: {}", e))?;
                let name = rel.to_string_lossy().to_string();
                zip.start_file(name, options)
                    .map_err(|e| format!("Failed to start file in zip: {}", e))?;
                let data = fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
                zip.write_all(&data)
                    .map_err(|e| format!("Failed to write to zip: {}", e))?;
            }
        }
    }

    zip.finish()
        .map_err(|e| format!("Failed to finalize zip: {}", e))?;
    Ok(BxValue::new_ptr(vm.string_new(dest_str)))
}

#[cfg(feature = "bif-zip")]
fn is_zip_format(format: &str) -> bool {
    let format = format.to_ascii_lowercase();
    format.is_empty() || format == "zip" || format.ends_with(".zip")
}

#[cfg(feature = "bif-zip")]
fn is_raw_bzip_format(format: &str) -> bool {
    matches!(format.to_ascii_lowercase().as_str(), "bzip" | "bzip2")
}

#[cfg(feature = "bif-zip")]
fn archive_format_from_path(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        "tgz".to_string()
    } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        "tbz2".to_string()
    } else if lower.ends_with(".tar.bz") || lower.ends_with(".tbz") {
        "tbz".to_string()
    } else if lower.ends_with(".tar") {
        "tar".to_string()
    } else if lower.ends_with(".bz2") {
        "bzip2".to_string()
    } else {
        "zip".to_string()
    }
}

#[cfg(feature = "bif-zip")]
fn build_tar_bytes(source: &Path, include_base_folder: bool) -> Result<Vec<u8>, String> {
    let mut builder = tar::Builder::new(Vec::new());
    if source.is_file() {
        let name = source
            .file_name()
            .ok_or_else(|| "Archive source has no file name".to_string())?;
        builder
            .append_path_with_name(source, name)
            .map_err(|e| format!("Failed to add archive entry: {}", e))?;
    } else if source.is_dir() {
        if include_base_folder {
            let name = source
                .file_name()
                .ok_or_else(|| "Archive source has no directory name".to_string())?;
            builder
                .append_dir_all(name, source)
                .map_err(|e| format!("Failed to add archive directory: {}", e))?;
        } else {
            for entry in walkdir::WalkDir::new(source)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();
                if path == source {
                    continue;
                }
                let name = path
                    .strip_prefix(source)
                    .map_err(|e| format!("Path error: {}", e))?;
                if path.is_dir() {
                    builder
                        .append_dir(name, path)
                        .map_err(|e| format!("Failed to add archive directory: {}", e))?;
                } else if path.is_file() {
                    builder
                        .append_path_with_name(path, name)
                        .map_err(|e| format!("Failed to add archive entry: {}", e))?;
                }
            }
        }
    }
    builder
        .into_inner()
        .map_err(|e| format!("Failed to finalize archive: {}", e))
}

#[cfg(feature = "bif-zip")]
fn compress_tar_bytes(bytes: Vec<u8>, format: &str) -> Result<Vec<u8>, String> {
    match format.to_ascii_lowercase().as_str() {
        "tar" => Ok(bytes),
        "tgz" | "gz" | "gzip" | "targz" => {
            use flate2::{Compression, write::GzEncoder};
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&bytes)
                .map_err(|e| format!("Failed to compress archive: {}", e))?;
            encoder
                .finish()
                .map_err(|e| format!("Failed to finalize archive: {}", e))
        }
        "tbz" | "tbz2" | "tar.bz" | "tar.bz2" | "bzip" | "bzip2" => {
            use bzip2::{Compression, write::BzEncoder};
            let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
            encoder
                .write_all(&bytes)
                .map_err(|e| format!("Failed to compress archive: {}", e))?;
            encoder
                .finish()
                .map_err(|e| format!("Failed to finalize archive: {}", e))
        }
        other => Err(format!("Unsupported archive format: {}", other)),
    }
}

#[cfg(feature = "bif-zip")]
fn compress_bzip_bytes(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    use bzip2::{Compression, write::BzEncoder};
    let mut encoder = BzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|e| format!("Failed to compress archive: {}", e))?;
    encoder
        .finish()
        .map_err(|e| format!("Failed to finalize archive: {}", e))
}

#[cfg(feature = "bif-zip")]
fn extract_tar_like(source: &str, destination: &str, format: &str) -> Result<(), String> {
    let bytes = fs::read(source).map_err(|e| format!("Failed to open archive: {}", e))?;
    let format = if format.is_empty() {
        archive_format_from_path(source)
    } else {
        format.to_ascii_lowercase()
    };
    if is_raw_bzip_format(&format) {
        use bzip2::read::BzDecoder;
        let mut decoded = Vec::new();
        BzDecoder::new(bytes.as_slice())
            .read_to_end(&mut decoded)
            .map_err(|e| format!("Failed to decompress archive: {}", e))?;
        fs::create_dir_all(destination)
            .map_err(|e| format!("Failed to create destination: {}", e))?;
        let name = Path::new(source)
            .file_stem()
            .ok_or_else(|| "Archive source has no file name".to_string())?;
        fs::write(Path::new(destination).join(name), decoded)
            .map_err(|e| format!("Failed to extract archive: {}", e))?;
        return Ok(());
    }
    let mut tar_bytes = Vec::new();
    match format.as_str() {
        "tar" => tar_bytes = bytes,
        "tgz" | "gz" | "gzip" | "targz" => {
            use flate2::read::GzDecoder;
            GzDecoder::new(bytes.as_slice())
                .read_to_end(&mut tar_bytes)
                .map_err(|e| format!("Failed to decompress archive: {}", e))?;
        }
        "tbz" | "tbz2" | "tar.bz" | "tar.bz2" | "bzip" | "bzip2" => {
            use bzip2::read::BzDecoder;
            BzDecoder::new(bytes.as_slice())
                .read_to_end(&mut tar_bytes)
                .map_err(|e| format!("Failed to decompress archive: {}", e))?;
        }
        other => return Err(format!("Unsupported archive format: {}", other)),
    }
    fs::create_dir_all(destination).map_err(|e| format!("Failed to create destination: {}", e))?;
    tar::Archive::new(tar_bytes.as_slice())
        .unpack(destination)
        .map_err(|e| format!("Failed to extract archive: {}", e))
}

#[cfg(feature = "bif-zip")]
pub fn zip_is_zip_file(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.is_empty() {
        return Err("isZipFile() expects 1 argument: (path)".to_string());
    }
    let path_str = vm.to_string(args[0]);
    let path = Path::new(&path_str);
    if !path.exists() {
        return Ok(BxValue::new_bool(false));
    }
    match fs::File::open(path) {
        Ok(file) => match zip::ZipArchive::new(file) {
            Ok(_) => Ok(BxValue::new_bool(true)),
            Err(_) => Ok(BxValue::new_bool(false)),
        },
        Err(_) => Ok(BxValue::new_bool(false)),
    }
}
