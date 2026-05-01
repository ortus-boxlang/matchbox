use std::{
    fs::File,
    io::{Seek, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LambdaArchitecture {
    Arm64,
    X86_64,
}

impl Default for LambdaArchitecture {
    fn default() -> Self {
        Self::Arm64
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BootstrapStubs<'a> {
    pub arm64: &'a [u8],
    pub x86_64: &'a [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFile {
    pub source: PathBuf,
    pub destination: PathBuf,
}

pub fn write_package_zip(
    input: impl AsRef<Path>,
    output: impl AsRef<Path>,
    architecture: LambdaArchitecture,
    stubs: BootstrapStubs<'_>,
) -> Result<Vec<PackageFile>> {
    let files = collect_package_files(input)?;
    let output = output.as_ref();
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let file =
        File::create(output).with_context(|| format!("failed to create {}", output.display()))?;
    write_package_zip_to_writer(file, &files, architecture, stubs)?;
    Ok(files)
}

pub fn write_package_zip_to_writer<W: Write + Seek>(
    writer: W,
    files: &[PackageFile],
    architecture: LambdaArchitecture,
    stubs: BootstrapStubs<'_>,
) -> Result<()> {
    let stub = match architecture {
        LambdaArchitecture::Arm64 => stubs.arm64,
        LambdaArchitecture::X86_64 => stubs.x86_64,
    };
    if stub.is_empty() {
        bail!("Lambda bootstrap stub for {architecture:?} is empty");
    }

    let mut zip = ZipWriter::new(writer);
    let executable_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o755);
    zip.start_file("bootstrap", executable_options)?;
    zip.write_all(stub)?;

    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for file in files {
        let destination = zip_path(&file.destination)?;
        zip.start_file(destination, file_options)?;
        let bytes = std::fs::read(&file.source)
            .with_context(|| format!("failed to read {}", file.source.display()))?;
        zip.write_all(&bytes)?;
    }

    zip.finish()?;
    Ok(())
}

pub fn collect_package_files(input: impl AsRef<Path>) -> Result<Vec<PackageFile>> {
    let input = input.as_ref();
    let mut files = if input.is_file() {
        collect_single_file_package(input)?
    } else if input.is_dir() {
        collect_directory_package(input)?
    } else {
        bail!("Lambda package input does not exist: {}", input.display());
    };

    files.sort_by(|left, right| left.destination.cmp(&right.destination));
    files.dedup_by(|left, right| left.destination == right.destination);
    Ok(files)
}

fn collect_single_file_package(input: &Path) -> Result<Vec<PackageFile>> {
    let root = input
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut files = Vec::new();

    files.push(PackageFile {
        source: input.to_path_buf(),
        destination: PathBuf::from("Lambda.bx"),
    });

    for entry in
        std::fs::read_dir(&root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path == input || !path.is_file() || !is_bx_file(&path) {
            continue;
        }
        let Some(name) = path.file_name().map(PathBuf::from) else {
            continue;
        };
        files.push(PackageFile {
            source: path,
            destination: name,
        });
    }

    add_if_file(&mut files, root.join("boxlang.json"), "boxlang.json");
    add_directory_recursive(
        &mut files,
        &root.join("boxlang_modules"),
        Path::new("boxlang_modules"),
    )?;
    Ok(files)
}

fn collect_directory_package(root: &Path) -> Result<Vec<PackageFile>> {
    let mut files = Vec::new();
    let source_root = root.join("src").join("main").join("bx");
    if source_root.is_dir() {
        add_directory_recursive(&mut files, &source_root, Path::new(""))?;
    } else {
        for entry in
            std::fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() || !is_bx_file(&path) {
                continue;
            }
            let Some(name) = path.file_name().map(PathBuf::from) else {
                continue;
            };
            files.push(PackageFile {
                source: path,
                destination: name,
            });
        }
    }

    add_if_file(
        &mut files,
        root.join("src").join("resources").join("boxlang.json"),
        "boxlang.json",
    );
    add_if_file(&mut files, root.join("boxlang.json"), "boxlang.json");

    let starter_modules = root.join("src").join("resources").join("boxlang_modules");
    if starter_modules.is_dir() {
        add_directory_recursive(&mut files, &starter_modules, Path::new("boxlang_modules"))?;
    } else {
        add_directory_recursive(
            &mut files,
            &root.join("boxlang_modules"),
            Path::new("boxlang_modules"),
        )?;
    }

    Ok(files)
}

fn add_if_file(files: &mut Vec<PackageFile>, source: PathBuf, destination: impl AsRef<Path>) {
    if source.is_file() {
        files.push(PackageFile {
            source,
            destination: destination.as_ref().to_path_buf(),
        });
    }
}

fn add_directory_recursive(
    files: &mut Vec<PackageFile>,
    source_root: &Path,
    destination_root: &Path,
) -> Result<()> {
    if !source_root.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(source_root)
        .with_context(|| format!("failed to read {}", source_root.display()))?
    {
        let entry = entry?;
        let source = entry.path();
        let destination = destination_root.join(entry.file_name());
        if source.is_dir() {
            add_directory_recursive(files, &source, &destination)?;
        } else if source.is_file() {
            files.push(PackageFile {
                source,
                destination,
            });
        }
    }

    Ok(())
}

fn is_bx_file(path: &Path) -> bool {
    matches!(path.extension().and_then(|ext| ext.to_str()), Some("bx"))
}

fn zip_path(path: &Path) -> Result<String> {
    let value = path.to_string_lossy().replace('\\', "/");
    if value.starts_with('/') || value.contains("../") || value == ".." {
        bail!("invalid package destination path: {}", path.display());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_single_file_with_sibling_support_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Handler.bx"), "class {}").unwrap();
        std::fs::write(dir.path().join("Application.bx"), "class {}").unwrap();
        std::fs::write(dir.path().join("Products.bx"), "class {}").unwrap();
        std::fs::write(dir.path().join("boxlang.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.path().join("boxlang_modules").join("demo")).unwrap();
        std::fs::write(
            dir.path()
                .join("boxlang_modules")
                .join("demo")
                .join("ModuleConfig.bx"),
            "class {}",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("ignored")).unwrap();
        std::fs::write(dir.path().join("ignored").join("Secret.bx"), "class {}").unwrap();

        let files = collect_package_files(dir.path().join("Handler.bx")).unwrap();
        let destinations = destinations(files);

        assert_eq!(
            destinations,
            vec![
                "Application.bx",
                "Lambda.bx",
                "Products.bx",
                "boxlang.json",
                "boxlang_modules/demo/ModuleConfig.bx",
            ]
        );
    }

    #[test]
    fn collects_starter_layout_files_flattened_to_root() {
        let dir = tempfile::tempdir().unwrap();
        let bx_dir = dir.path().join("src").join("main").join("bx");
        std::fs::create_dir_all(&bx_dir).unwrap();
        std::fs::write(bx_dir.join("Lambda.bx"), "class {}").unwrap();
        std::fs::write(bx_dir.join("Products.bx"), "class {}").unwrap();
        std::fs::create_dir_all(bx_dir.join("models")).unwrap();
        std::fs::write(bx_dir.join("models").join("User.bx"), "class {}").unwrap();
        let resources = dir.path().join("src").join("resources");
        std::fs::create_dir_all(resources.join("boxlang_modules").join("demo")).unwrap();
        std::fs::write(resources.join("boxlang.json"), "{}").unwrap();
        std::fs::write(
            resources
                .join("boxlang_modules")
                .join("demo")
                .join("ModuleConfig.bx"),
            "class {}",
        )
        .unwrap();

        let files = collect_package_files(dir.path()).unwrap();
        let destinations = destinations(files);

        assert_eq!(
            destinations,
            vec![
                "Lambda.bx",
                "Products.bx",
                "boxlang.json",
                "boxlang_modules/demo/ModuleConfig.bx",
                "models/User.bx",
            ]
        );
    }

    #[test]
    fn writes_zip_with_selected_bootstrap_and_package_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        std::fs::write(dir.path().join("Products.bx"), "class {}").unwrap();
        let out = dir.path().join("dist").join("lambda.zip");

        let files = write_package_zip(
            dir.path().join("Lambda.bx"),
            &out,
            LambdaArchitecture::Arm64,
            BootstrapStubs {
                arm64: b"arm64-bootstrap",
                x86_64: b"x86-bootstrap",
            },
        )
        .unwrap();

        assert_eq!(destinations(files), vec!["Lambda.bx", "Products.bx"]);

        let file = File::open(out).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut names = archive.file_names().map(str::to_string).collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, vec!["Lambda.bx", "Products.bx", "bootstrap"]);

        let bootstrap = archive.by_name("bootstrap").unwrap();
        assert_eq!(bootstrap.unix_mode().unwrap() & 0o777, 0o755);
    }

    #[test]
    fn rejects_empty_selected_bootstrap_stub() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Lambda.bx"), "class {}").unwrap();
        let mut buffer = std::io::Cursor::new(Vec::new());
        let err = write_package_zip_to_writer(
            &mut buffer,
            &collect_package_files(dir.path().join("Lambda.bx")).unwrap(),
            LambdaArchitecture::X86_64,
            BootstrapStubs {
                arm64: b"arm",
                x86_64: b"",
            },
        )
        .unwrap_err()
        .to_string();

        assert!(err.contains("empty"));
    }

    fn destinations(files: Vec<PackageFile>) -> Vec<String> {
        files
            .into_iter()
            .map(|file| file.destination.to_string_lossy().replace('\\', "/"))
            .collect()
    }
}
