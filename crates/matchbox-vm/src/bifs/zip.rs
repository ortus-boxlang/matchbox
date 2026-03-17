#[cfg(feature = "bif-zip")]
use crate::types::{BxValue, BxVM};
#[cfg(feature = "bif-zip")]
use std::fs;
#[cfg(feature = "bif-zip")]
use std::path::Path;

#[cfg(feature = "bif-zip")]
pub fn zip_extract(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    if args.len() < 2 { return Err("extract() expects 2 arguments: (zip_file, dest_dir)".to_string()); }
    let zip_file_str = vm.to_string(args[0]);
    let dest_dir_str = vm.to_string(args[1]);
    
    let file = fs::File::open(&zip_file_str).map_err(|e| format!("Failed to open zip file: {}", e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;
    
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("Failed to read file from zip: {}", e))?;
        let outpath = match file.enclosed_name() {
            Some(path) => Path::new(&dest_dir_str).join(path),
            None => continue,
        };

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath).map_err(|e| format!("Failed to create directory: {}", e))?;
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).map_err(|e| format!("Failed to create directory: {}", e))?;
                }
            }
            let mut outfile = fs::File::create(&outpath).map_err(|e| format!("Failed to create file: {}", e))?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| format!("Failed to copy file: {}", e))?;
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
