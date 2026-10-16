//! Buffered HTTP multipart parsing with request-owned, securely created temporary files.
use crate::webroot_core::WebrootRequest;
use anyhow::{Context, Result, ensure};
use std::{path::Path, sync::Arc};
use tokio::io::AsyncWriteExt;
#[derive(Clone, Debug)]
pub struct UploadedFile {
    pub field: String,
    pub filename: String,
    pub content_type: String,
    pub temporary: Arc<tempfile::TempPath>,
}
impl UploadedFile {
    pub fn path(&self) -> &Path {
        self.temporary.as_ref().as_ref()
    }
}
pub(crate) struct Cleanup(pub Vec<UploadedFile>);
impl Drop for Cleanup {
    fn drop(&mut self) {
        for file in &self.0 {
            if let Err(error) = std::fs::remove_file(file.path())
                && error.kind() != std::io::ErrorKind::NotFound
            {
                eprintln!("Removing upload temporary file: {error}");
            }
        }
    }
}
/// The HTTP host enforces its entity limit before calling this parser. No file survives the request owner.
pub async fn parse(request: &mut WebrootRequest) -> Result<()> {
    let Some(content_type) = request.headers.get("content-type") else {
        return Ok(());
    };
    if !content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("multipart/form-data")
    {
        return Ok(());
    }
    let boundary = multer::parse_boundary(content_type).context("Invalid multipart boundary")?;
    ensure!(
        !boundary.is_empty() && boundary.len() <= 70,
        "Multipart boundary exceeds RFC limit"
    );
    let data = axum::body::Bytes::copy_from_slice(&request.body);
    let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(data) });
    let mut multipart = multer::Multipart::new(stream, boundary);
    let mut fields = std::collections::HashMap::<String, String>::new();
    let mut arrays = std::collections::HashMap::<String, Vec<String>>::new();
    let mut uploads = Vec::new();
    let mut count = 0;
    while let Some(field) = multipart
        .next_field()
        .await
        .context("Malformed multipart data")?
    {
        count += 1;
        ensure!(count <= 1000, "Too many multipart fields");
        let name = field
            .name()
            .context("Multipart field has no name")?
            .to_string();
        let filename = field.file_name().map(str::to_string);
        let mime = field
            .content_type()
            .map(ToString::to_string)
            .unwrap_or_else(|| "application/octet-stream".into());
        let bytes = field.bytes().await.context("Malformed multipart field")?;
        let empty_file = filename.as_ref().is_some_and(String::is_empty);
        let value = if let Some(filename) = filename.filter(|name| !name.is_empty()) {
            let file = tempfile::Builder::new()
                .prefix("matchbox-upload-")
                .tempfile()
                .context("Creating upload temporary file")?;
            let mut writer = tokio::fs::File::from_std(
                file.as_file()
                    .try_clone()
                    .context("Opening upload temporary file")?,
            );
            writer
                .write_all(&bytes)
                .await
                .context("Writing upload temporary file")?;
            writer
                .flush()
                .await
                .context("Flushing upload temporary file")?;
            let path = Arc::new(file.into_temp_path());
            let value = path.to_string_lossy().into_owned();
            uploads.push(UploadedFile {
                field: name.clone(),
                filename,
                content_type: mime,
                temporary: path,
            });
            value
        } else if empty_file {
            String::new()
        } else {
            String::from_utf8(bytes.to_vec()).context("Multipart text must be UTF-8")?
        };
        if let Some(name) = name.strip_suffix("[]") {
            arrays.entry(name.into()).or_default().push(value);
        } else {
            fields
                .entry(name)
                .and_modify(|previous| {
                    if !value.is_empty() {
                        if !previous.is_empty() {
                            previous.push(',');
                        }
                        previous.push_str(&value)
                    }
                })
                .or_insert(value);
        }
    }
    request.form = fields;
    request.form_arrays = arrays;
    request.uploads = uploads;
    Ok(())
}
