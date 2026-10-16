use super::application_runtime::RenderJob;
pub use super::application_runtime::WebrootRuntime;
use anyhow::{Context, Result, bail};
use matchbox_compiler::{compiler::Compiler, parser};
use matchbox_vm::types::{BxVM, BxValue};
use matchbox_vm::vm::VM;
use matchbox_vm::vm::chunk::Chunk;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WebrootConfig {
    pub rewrites: bool,
    pub rewrite_file_name: String,
}

impl Default for WebrootConfig {
    fn default() -> Self {
        Self {
            rewrites: false,
            rewrite_file_name: "index.bxm".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct WebrootRequest {
    pub method: String,
    pub path: String,
    pub query: HashMap<String, String>,
    pub form: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Transport metadata supplied by an HTTP host, separate from the resolved asset path.
#[derive(Clone, Debug)]
pub struct WebrootContext {
    pub uri: String,
    pub script_name: String,
    pub query_string: String,
    pub protocol: String,
    pub scheme: String,
    pub remote_addr: String,
    pub remote_port: u16,
    pub server_port: u16,
}

impl Default for WebrootContext {
    fn default() -> Self {
        Self {
            uri: String::new(),
            script_name: String::new(),
            query_string: String::new(),
            protocol: "HTTP/1.1".into(),
            scheme: "http".into(),
            remote_addr: String::new(),
            remote_port: 0,
            server_port: 8080,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebrootResponse {
    pub status: u16,
    /// Repeated fields (including Set-Cookie) remain separate and ordered.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub trait AssetStore {
    fn metadata(&self, relative_path: &str) -> Result<Option<AssetMetadata>>;
    fn read(&self, relative_path: &str) -> Result<Option<Vec<u8>>>;

    fn compiled_template(&self, _relative_path: &str) -> Result<Option<Chunk>> {
        Ok(None)
    }

    fn source_path(&self, relative_path: &str) -> Result<String> {
        Ok(relative_path.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetMetadata {
    pub is_dir: bool,
}

#[derive(Clone, Debug)]
pub struct FileSystemAssetStore {
    root: PathBuf,
}

impl FileSystemAssetStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn full_path(&self, relative_path: &str) -> Result<PathBuf> {
        let mut full = self.root.clone();
        if !relative_path.is_empty() {
            for component in Path::new(relative_path).components() {
                match component {
                    Component::Normal(part) => full.push(part),
                    Component::CurDir => {}
                    _ => bail!("Invalid webroot path"),
                }
            }
        }
        Ok(full)
    }
}

impl AssetStore for FileSystemAssetStore {
    fn source_path(&self, relative_path: &str) -> Result<String> {
        Ok(self
            .full_path(relative_path)?
            .to_string_lossy()
            .into_owned())
    }

    fn metadata(&self, relative_path: &str) -> Result<Option<AssetMetadata>> {
        let full = self.full_path(relative_path)?;
        match std::fs::metadata(full) {
            Ok(metadata) => Ok(Some(AssetMetadata {
                is_dir: metadata.is_dir(),
            })),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn read(&self, relative_path: &str) -> Result<Option<Vec<u8>>> {
        let full = self.full_path(relative_path)?;
        match std::fs::read(full) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EmbeddedAssetStore {
    files: HashMap<String, Vec<u8>>,
    compiled_templates: HashMap<String, Chunk>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct EmbeddedAssetPackage {
    pub files: HashMap<String, Vec<u8>>,
    pub compiled_templates: HashMap<String, Chunk>,
}

impl EmbeddedAssetStore {
    pub fn new(files: HashMap<String, Vec<u8>>) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|(path, bytes)| (normalize_embedded_asset_path(&path), bytes))
                .collect(),
            compiled_templates: HashMap::new(),
        }
    }

    pub fn from_directory(root: &Path) -> Result<Self> {
        let mut files = HashMap::new();
        collect_embedded_files(root, root, &mut files)?;
        let mut compiled_templates = HashMap::new();
        for (path, bytes) in &files {
            if is_template_path(path) {
                let source = String::from_utf8(bytes.clone())?;
                compiled_templates.insert(
                    path.clone(),
                    compile_template(path, &source)
                        .with_context(|| format!("Failed to compile embedded template {path}"))?,
                );
            }
        }

        Ok(Self {
            files,
            compiled_templates,
        })
    }

    pub fn into_package(self) -> EmbeddedAssetPackage {
        EmbeddedAssetPackage {
            files: self.files,
            compiled_templates: self.compiled_templates,
        }
    }

    pub fn from_package(package: EmbeddedAssetPackage) -> Self {
        Self {
            files: package
                .files
                .into_iter()
                .map(|(path, bytes)| (normalize_embedded_asset_path(&path), bytes))
                .collect(),
            compiled_templates: package
                .compiled_templates
                .into_iter()
                .map(|(path, chunk)| (normalize_embedded_asset_path(&path), chunk))
                .collect(),
        }
    }
}

impl AssetStore for EmbeddedAssetStore {
    fn metadata(&self, relative_path: &str) -> Result<Option<AssetMetadata>> {
        let path = normalize_embedded_asset_path(relative_path);
        if self.files.contains_key(&path) {
            return Ok(Some(AssetMetadata { is_dir: false }));
        }

        let prefix = if path.is_empty() {
            String::new()
        } else {
            format!("{}/", path)
        };
        if self.files.keys().any(|key| key.starts_with(&prefix)) {
            return Ok(Some(AssetMetadata { is_dir: true }));
        }

        Ok(None)
    }

    fn read(&self, relative_path: &str) -> Result<Option<Vec<u8>>> {
        Ok(self
            .files
            .get(&normalize_embedded_asset_path(relative_path))
            .cloned())
    }

    fn compiled_template(&self, relative_path: &str) -> Result<Option<Chunk>> {
        Ok(self
            .compiled_templates
            .get(&normalize_embedded_asset_path(relative_path))
            .cloned())
    }
}

pub struct WebrootEngine<S> {
    store: Arc<S>,
    config: WebrootConfig,
    sessions: Mutex<HashMap<String, HashMap<String, String>>>,
    runtime: WebrootRuntime,
    scope_id: String,
}

impl<S: AssetStore + Send + Sync + 'static> WebrootEngine<S> {
    pub fn new(store: S, config: WebrootConfig) -> Self {
        Self::with_runtime(store, config, WebrootRuntime::default())
    }

    pub fn with_runtime(store: S, config: WebrootConfig, runtime: WebrootRuntime) -> Self {
        Self {
            store: Arc::new(store),
            config,
            runtime,
            sessions: Mutex::new(HashMap::new()),
            scope_id: uuid::Uuid::new_v4().to_string(),
        }
    }

    pub fn handle(&self, request: WebrootRequest) -> Result<WebrootResponse> {
        self.handle_with_context(request, WebrootContext::default())
    }

    pub fn handle_with_context(
        &self,
        request: WebrootRequest,
        context: WebrootContext,
    ) -> Result<WebrootResponse> {
        let method = request.method.to_ascii_uppercase();
        if matches!(method.as_str(), "TRACE" | "TRACK") {
            return Ok(text_response(405, "Method Not Allowed"));
        }

        let relative_path = match normalize_request_path(&request.path) {
            Ok(path) => path,
            Err(_) => {
                return Ok(text_response(
                    403,
                    "Forbidden: Directory traversal attempt.",
                ));
            }
        };
        if contains_hidden_segment(&relative_path) {
            return Ok(text_response(
                403,
                "Forbidden: Access to hidden files is denied.",
            ));
        }

        let mut asset_path = relative_path;
        if self
            .store
            .metadata(&asset_path)?
            .map(|metadata| metadata.is_dir)
            .unwrap_or(false)
        {
            asset_path =
                first_existing_index(self.store.as_ref(), &asset_path)?.unwrap_or(asset_path);
        }

        let extension = Path::new(&asset_path)
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(extension.as_str(), "bx" | "cfm" | "cfc" | "cfs" | "cfml") {
            return Ok(text_response(501, "Unsupported executable source type"));
        }
        let bytes = self.read_or_rewrite(&mut asset_path)?;
        let mut response = if is_template_path(&asset_path) {
            self.render_template(&asset_path, request, context)?
        } else {
            let Some(bytes) = bytes else {
                return Ok(text_response(404, "Not Found"));
            };
            if !matches!(method.as_str(), "GET" | "HEAD" | "POST") {
                return Ok(text_response(405, "Method Not Allowed"));
            }
            let headers = vec![(
                "content-type".to_string(),
                mime_guess::from_path(&asset_path)
                    .first_or_octet_stream()
                    .to_string(),
            )];
            WebrootResponse {
                status: 200,
                headers,
                body: bytes,
            }
        };

        if !response
            .headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        {
            response
                .headers
                .push(("content-length".into(), response.body.len().to_string()));
        }
        if response.status == 204 {
            response
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
        }
        if method == "HEAD" || response.status == 204 || response.status == 304 {
            response.body.clear();
        }
        Ok(response)
    }

    fn read_or_rewrite(&self, asset_path: &mut String) -> Result<Option<Vec<u8>>> {
        if let Some(bytes) = self.store.read(asset_path)? {
            return Ok(Some(bytes));
        }

        if self.config.rewrites {
            let rewrite_path = normalize_request_path(&self.config.rewrite_file_name)?;
            *asset_path = rewrite_path;
            return self.store.read(asset_path);
        }

        Ok(None)
    }

    fn render_template(
        &self,
        asset_path: &str,
        request: WebrootRequest,
        context: WebrootContext,
    ) -> Result<WebrootResponse> {
        let cookies = request
            .headers
            .get("cookie")
            .map(|raw| parse_cookie_header(raw))
            .unwrap_or_default();
        let session_id = cookies
            .get("MBX_SESSION_ID")
            .cloned()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let session = self
            .sessions
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default();
        let result = self.runtime.render(RenderJob {
            store: self.store.clone(),
            asset_path: asset_path.into(),
            scope_id: self.scope_id.clone(),
            request,
            context,
            session_id: session_id.clone(),
            session,
        })?;
        if let Some(session) = result.session {
            self.sessions.lock().unwrap().insert(session_id, session);
        }
        Ok(result.response)
    }
}

pub(crate) fn insert_scope(vm: &mut VM, name: &str, values: &HashMap<String, String>) -> usize {
    let scope_id = vm.struct_new();
    vm.insert_global(name.to_string(), BxValue::new_ptr(scope_id));
    for (key, value) in values {
        let value_id = vm.string_new(value.clone());
        vm.struct_set(scope_id, key, BxValue::new_ptr(value_id));
    }
    scope_id
}

pub(crate) fn parse_cookie_header(raw: &str) -> HashMap<String, String> {
    let mut cookies = HashMap::new();
    for part in raw.split(';') {
        let mut kv = part.splitn(2, '=');
        if let (Some(key), Some(value)) = (kv.next(), kv.next()) {
            cookies.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    cookies
}

pub(crate) fn form_scope_from_request(request: &WebrootRequest) -> HashMap<String, String> {
    if !request.form.is_empty() {
        return request.form.clone();
    }

    if request.method.eq_ignore_ascii_case("POST")
        && request
            .headers
            .get("content-type")
            .map(|value| {
                value
                    .split(';')
                    .next()
                    .unwrap_or(value)
                    .trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            })
            .unwrap_or(false)
    {
        return url::form_urlencoded::parse(&request.body)
            .into_owned()
            .collect();
    }

    HashMap::new()
}

pub(crate) fn compile_template(asset_path: &str, source: &str) -> Result<Chunk> {
    let ast = if asset_path.ends_with(".bxm") {
        parser::parse_bxm(source, Some(asset_path))?
    } else {
        parser::parse(source, Some(asset_path))?
    };
    let mut compiler = Compiler::new(asset_path);
    compiler.compile(&ast, source)
}

pub(crate) fn cgi_scope_from_request(
    request: &WebrootRequest,
    asset_path: &str,
    context: &WebrootContext,
) -> HashMap<String, String> {
    let host = request
        .headers
        .get("host")
        .cloned()
        .unwrap_or_else(|| "localhost:8080".to_string());
    let authority = host.parse::<http::uri::Authority>().ok();
    let server_name = authority
        .as_ref()
        .map(|a| a.host().trim_matches(['[', ']']).to_string())
        .unwrap_or_else(|| host.clone());
    let server_port = authority
        .as_ref()
        .and_then(|a| a.port_u16())
        .unwrap_or(context.server_port)
        .to_string();
    let uri = if context.uri.is_empty() {
        &request.path
    } else {
        &context.uri
    };
    let script_name = if context.script_name.is_empty() {
        format!("/{asset_path}")
    } else {
        context.script_name.clone()
    };
    let mut cgi = HashMap::from([
        (
            "request_method".to_string(),
            request.method.to_ascii_uppercase(),
        ),
        ("script_name".to_string(), script_name),
        ("path_info".to_string(), String::new()),
        ("query_string".to_string(), context.query_string.clone()),
        (
            "request_url".to_string(),
            format!("{}://{}{}", context.scheme, host, uri),
        ),
        ("server_protocol".to_string(), context.protocol.clone()),
        ("remote_addr".to_string(), context.remote_addr.clone()),
        ("remote_host".to_string(), context.remote_addr.clone()),
        ("remote_port".to_string(), context.remote_port.to_string()),
        (
            "https".to_string(),
            if context.scheme == "https" {
                "on"
            } else {
                "off"
            }
            .into(),
        ),
        ("server_name".to_string(), server_name),
        ("server_port".to_string(), server_port),
        ("http_host".to_string(), host),
        (
            "content_type".to_string(),
            request
                .headers
                .get("content-type")
                .cloned()
                .unwrap_or_default(),
        ),
        ("content_length".to_string(), request.body.len().to_string()),
    ]);
    for (name, value) in &request.headers {
        cgi.insert(
            format!("http_{}", name.to_ascii_lowercase().replace('-', "_")),
            value.clone(),
        );
    }
    cgi
}

fn normalize_request_path(path: &str) -> Result<String> {
    let path = path
        .split('?')
        .next()
        .unwrap_or(path)
        .trim_start_matches('/');
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().to_string()),
            Component::CurDir => {}
            Component::RootDir => {}
            _ => bail!("Invalid webroot path"),
        }
    }
    Ok(parts.join("/"))
}

fn normalize_embedded_asset_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

fn collect_embedded_files(
    root: &Path,
    current: &Path,
    files: &mut HashMap<String, Vec<u8>>,
) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_embedded_files(root, &path, files)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            files.insert(relative, std::fs::read(&path)?);
        }
    }
    Ok(())
}

fn contains_hidden_segment(path: &str) -> bool {
    path.split('/')
        .any(|part| part.starts_with('.') && !part.is_empty())
}

fn first_existing_index<S: AssetStore>(store: &S, dir: &str) -> Result<Option<String>> {
    for index in ["index.bxm", "index.bxs"] {
        let candidate = if dir.is_empty() {
            index.to_string()
        } else {
            format!("{}/{}", dir, index)
        };
        if store.metadata(&candidate)?.is_some() {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn is_template_path(path: &str) -> bool {
    path.ends_with(".bxm") || path.ends_with(".bxs")
}

pub(crate) fn text_response(status: u16, body: &str) -> WebrootResponse {
    WebrootResponse {
        status,
        headers: if status == 405 {
            vec![("allow".into(), "GET, HEAD, POST".into())]
        } else {
            Vec::new()
        },
        body: body.as_bytes().to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_static_asset_returns_content_type_and_body() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(webroot.join("styles.css"), "body { color: red; }").unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/styles.css".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("text/css")
        );
        assert_eq!(response.body, b"body { color: red; }");
    }

    #[test]
    fn get_root_renders_index_template() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<h1>Index</h1>").unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("text/html")
        );
        assert_eq!(String::from_utf8(response.body).unwrap(), "<h1>Index</h1>");
    }

    #[test]
    fn embedded_asset_store_serves_same_root_template_behavior() {
        let engine = WebrootEngine::new(
            EmbeddedAssetStore::new(HashMap::from([(
                "index.bxm".to_string(),
                b"<bx:output>Embedded</bx:output>".to_vec(),
            )])),
            WebrootConfig::default(),
        );

        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), "Embedded");
    }

    #[test]
    fn embedded_asset_store_can_be_built_from_filesystem_webroot() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path();
        std::fs::create_dir(webroot.join("assets")).unwrap();
        std::fs::write(webroot.join("index.bxm"), "<bx:output>Home</bx:output>").unwrap();
        std::fs::write(webroot.join("assets").join("styles.css"), "body {}").unwrap();

        let store = EmbeddedAssetStore::from_directory(webroot).unwrap();
        let engine = WebrootEngine::new(store, WebrootConfig::default());

        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/assets/styles.css".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"body {}");
    }

    #[test]
    fn embedded_asset_store_rejects_invalid_templates_during_packaging() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path();
        std::fs::write(webroot.join("broken.bxs"), "if (").unwrap();

        let err = EmbeddedAssetStore::from_directory(webroot).unwrap_err();

        assert!(err.to_string().contains("broken.bxs"));
    }

    #[test]
    fn template_can_read_query_params_from_url_scope() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("hello.bxm"),
            "<bx:output>Hello #url.name#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/hello.bxm".to_string(),
                query: HashMap::from([("name".to_string(), "MatchBox".to_string())]),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), "Hello MatchBox");
    }

    #[test]
    fn template_can_read_posted_form_scope() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("submit.bxm"),
            "<bx:output>Posted #form.name#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "POST".to_string(),
                path: "/submit.bxm".to_string(),
                query: Default::default(),
                form: HashMap::from([("name".to_string(), "FormValue".to_string())]),
                headers: Default::default(),
                body: b"name=FormValue".to_vec(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "Posted FormValue"
        );
    }

    #[test]
    fn urlencoded_post_body_populates_form_scope() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("submit.bxm"),
            "<bx:output>#form.name#/#form.city#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "POST".to_string(),
                path: "/submit.bxm".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: HashMap::from([(
                    "content-type".to_string(),
                    "application/x-www-form-urlencoded".to_string(),
                )]),
                body: b"name=Encoded+Value&city=San%20Antonio".to_vec(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "Encoded Value/San Antonio"
        );
    }

    #[test]
    fn trace_method_returns_405() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(webroot.join("index.bxm"), "<h1>Index</h1>").unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "TRACE".to_string(),
                path: "/".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 405);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "Method Not Allowed"
        );
    }

    #[test]
    fn head_returns_headers_without_body() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(webroot.join("styles.css"), "body { color: red; }").unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "HEAD".to_string(),
                path: "/styles.css".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            response
                .headers
                .iter()
                .find(|(name, _)| name == "content-type")
                .map(|(_, value)| value.as_str()),
            Some("text/css")
        );
        assert!(response.body.is_empty());
    }

    #[test]
    fn hidden_paths_are_rejected_by_core() {
        let engine = WebrootEngine::new(
            EmbeddedAssetStore::new(HashMap::from([(
                ".env".to_string(),
                b"SECRET=value".to_vec(),
            )])),
            WebrootConfig::default(),
        );

        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/.env".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 403);
    }

    #[test]
    fn traversal_paths_are_rejected_by_core() {
        let engine = WebrootEngine::new(EmbeddedAssetStore::default(), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/../outside.txt".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 403);
    }

    #[test]
    fn rewrites_render_configured_template_for_missing_path() {
        let engine = WebrootEngine::new(
            EmbeddedAssetStore::new(HashMap::from([(
                "index.bxm".to_string(),
                b"<bx:output>rewrite</bx:output>".to_vec(),
            )])),
            WebrootConfig {
                rewrites: true,
                rewrite_file_name: "index.bxm".to_string(),
            },
        );

        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/missing/route".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), "rewrite");
    }

    #[test]
    fn template_can_read_cookies_from_cookie_scope() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("cookie.bxm"),
            "<bx:output>#cookie.theme#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/cookie.bxm".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: HashMap::from([("cookie".to_string(), "theme=dark".to_string())]),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(String::from_utf8(response.body).unwrap(), "dark");
    }

    #[test]
    fn template_can_read_synthesized_cgi_scope() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("cgi.bxm"),
            "<bx:output>#cgi.request_method# #cgi.script_name# #cgi.server_name#:#cgi.server_port#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let response = engine
            .handle(WebrootRequest {
                method: "POST".to_string(),
                path: "/cgi.bxm".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: HashMap::from([("host".to_string(), "example.test:9090".to_string())]),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(response.status, 200);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            "POST /cgi.bxm example.test:9090"
        );
    }

    #[test]
    fn session_scope_persists_for_subsequent_request_with_session_cookie() {
        let temp = tempfile::tempdir().unwrap();
        let webroot = temp.path().canonicalize().unwrap();
        std::fs::write(
            webroot.join("set.bxm"),
            "<bx:set session.favorite = \"blue\"><bx:output>set</bx:output>",
        )
        .unwrap();
        std::fs::write(
            webroot.join("read.bxm"),
            "<bx:output>#session.favorite#</bx:output>",
        )
        .unwrap();

        let engine =
            WebrootEngine::new(FileSystemAssetStore::new(webroot), WebrootConfig::default());
        let first = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/set.bxm".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: Default::default(),
                body: Vec::new(),
            })
            .unwrap();
        let session_cookie = first
            .headers
            .iter()
            .find(|(name, _)| name == "set-cookie")
            .map(|(_, value)| value)
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();

        let second = engine
            .handle(WebrootRequest {
                method: "GET".to_string(),
                path: "/read.bxm".to_string(),
                query: Default::default(),
                form: Default::default(),
                headers: HashMap::from([("cookie".to_string(), session_cookie)]),
                body: Vec::new(),
            })
            .unwrap();

        assert_eq!(String::from_utf8(second.body).unwrap(), "blue");
    }
}
