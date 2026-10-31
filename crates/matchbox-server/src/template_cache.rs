//! Worker-owned immutable template programs; never cache response or request values.
use crate::webroot_core::AssetStore;
use anyhow::Result;
use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::vm::chunk::Chunk;
use std::{collections::VecDeque, io};

const MAX_ENTRIES: usize = 64;
const MAX_BYTES: usize = 16 * 1024 * 1024;

type Key = (String, String, bool, bool);
struct Entry {
    key: Key,
    filename: String,
    source: String,
    chunk: Chunk,
    weight: usize,
}
#[derive(Default)]
pub(crate) struct TemplateCache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

struct Weight(usize);
impl io::Write for Weight {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_BYTES.saturating_sub(self.0) {
            return Err(io::Error::other("Template cache entry exceeds budget"));
        }
        self.0 += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl TemplateCache {
    pub(crate) fn load(
        &mut self,
        store: &dyn AssetStore,
        scope: &str,
        path: &str,
        class_context: bool,
        module_classes: bool,
    ) -> Result<Option<Chunk>> {
        if !class_context && let Some(chunk) = store.compiled_template(path)? {
            return Ok(Some(chunk));
        }
        let key = (
            scope.to_owned(),
            path.to_owned(),
            class_context,
            module_classes,
        );
        let old = self
            .entries
            .iter()
            .position(|entry| entry.key == key)
            .map(|index| {
                let entry = self.entries.remove(index).unwrap();
                self.bytes -= entry.weight;
                entry
            });
        // Re-read through the store on EVERY load: permissions, confinement, deletion
        // and byte changes (even same-size/same-mtime edits) cannot be hidden by a hit.
        let Some(bytes) = store.read(path)? else {
            return Ok(None);
        };
        let filename = store.source_path(path)?;
        if let Some(entry) = old
            && entry.filename == filename
            && entry.source.as_bytes() == bytes
        {
            let chunk = entry.chunk.clone_without_runtime_caches();
            self.bytes += entry.weight;
            self.entries.push_back(entry);
            return Ok(Some(chunk));
        }
        let source = String::from_utf8(bytes)?;
        let ast = if path.ends_with(".bxm") {
            parser::parse_bxm(&source, Some(&filename))?
        } else {
            parser::parse(&source, Some(&filename))?
        };
        let mut compiler = Compiler::new(&filename);
        compiler.runtime_module_classes = module_classes;
        let chunk = if class_context {
            compiler.compile_class_template(&ast, &source)?
        } else {
            compiler.compile(&ast, &source)?
        };
        // Budget serialized program weight plus retained source/key bytes, not a
        // claim of exact resident memory. Oversized programs execute uncached.
        let mut weight = Weight(source.len() + filename.len() + scope.len() + path.len());
        // shortcut: source dependencies execute uncached, upgrade when dependency-graph invalidation exists.
        if !compiler.has_source_dependencies()
            && weight.0 <= MAX_BYTES
            && serde_json::to_writer(&mut weight, &chunk).is_ok()
        {
            while self.entries.len() >= MAX_ENTRIES || self.bytes + weight.0 > MAX_BYTES {
                let entry = self.entries.pop_front().unwrap();
                self.bytes -= entry.weight;
            }
            self.bytes += weight.0;
            self.entries.push_back(Entry {
                key,
                filename,
                source,
                chunk: chunk.clone_without_runtime_caches(),
                weight: weight.0,
            });
        }
        Ok(Some(chunk))
    }
}
