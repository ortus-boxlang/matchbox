use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedListenConfig {
    pub host: String,
    pub port: u16,
}

impl Default for EmbeddedListenConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".to_string(),
            port: 8080,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedRoute {
    pub method: String,
    pub path: String,
    pub source_path: PathBuf,
    pub source_kind: EmbeddedSourceKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddedSourceKind {
    Template,
    Script,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedAppDefinition {
    pub listen: EmbeddedListenConfig,
    pub routes: Vec<EmbeddedRoute>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedRequest {
    pub method: String,
    pub path: String,
    pub url: HashMap<String, String>,
    pub form: HashMap<String, String>,
    pub request: HashMap<String, String>,
    pub cgi: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl EmbeddedRequest {
    pub fn with_route_and_query(
        method: impl Into<String>,
        path: impl Into<String>,
        route_params: HashMap<String, String>,
        query_params: HashMap<String, String>,
    ) -> Self {
        let mut url = query_params;
        for (key, value) in route_params {
            url.insert(key, value);
        }

        Self {
            method: method.into(),
            path: path.into(),
            url,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbeddedResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Default for EmbeddedResponse {
    fn default() -> Self {
        Self {
            status: 200,
            headers: HashMap::new(),
            body: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedRouteMatch<'a> {
    pub route: &'a EmbeddedRoute,
    pub params: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbeddedRuntimeProfile {
    pub name: &'static str,
    pub supports_compile_time_templates: bool,
    pub supports_static_files: bool,
    pub supports_sessions: bool,
    pub request_scopes: &'static [&'static str],
}

pub const ESP32_PROFILE: EmbeddedRuntimeProfile = EmbeddedRuntimeProfile {
    name: "esp32",
    supports_compile_time_templates: true,
    supports_static_files: false,
    supports_sessions: false,
    request_scopes: &["url", "form", "request", "cgi"],
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmbeddedHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl EmbeddedHttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }

    pub fn from_suffix(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "get" => Some(Self::Get),
            "post" => Some(Self::Post),
            "put" => Some(Self::Put),
            "patch" => Some(Self::Patch),
            "delete" => Some(Self::Delete),
            "head" => Some(Self::Head),
            "options" => Some(Self::Options),
            _ => None,
        }
    }
}

pub fn normalize_route_path(path: &str) -> String {
    if path == "/" || path.trim().is_empty() {
        return "/".to_string();
    }

    let trimmed = path.trim().trim_matches('/');
    format!("/{}", trimmed)
}

pub fn join_route_paths(prefix: &str, path: &str) -> String {
    let prefix = normalize_route_path(prefix);
    let path = normalize_route_path(path);

    if prefix == "/" {
        return path;
    }

    if path == "/" {
        return prefix;
    }

    format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

pub fn route_from_app_file(app_root: &Path, source_path: &Path) -> Result<EmbeddedRoute> {
    let relative = source_path.strip_prefix(app_root).map_err(|error| {
        anyhow::anyhow!(
            "Embedded route source '{}' is not inside app root '{}': {}",
            source_path.display(),
            app_root.display(),
            error
        )
    })?;

    let extension = source_path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let source_kind = match extension.as_str() {
        "bxm" => EmbeddedSourceKind::Template,
        "bxs" => EmbeddedSourceKind::Script,
        _ => bail!(
            "Embedded route source '{}' must end in .bxm or .bxs",
            source_path.display()
        ),
    };

    let mut method = EmbeddedHttpMethod::Get;
    let mut segments = Vec::new();

    for component in relative.components() {
        let raw = component.as_os_str().to_string_lossy();
        let component = raw.as_ref();

        if component == "." || component.is_empty() {
            continue;
        }

        let is_last = component == relative.file_name().and_then(|x| x.to_str()).unwrap_or_default();
        if is_last {
            let stem = Path::new(component)
                .file_stem()
                .and_then(|x| x.to_str())
                .unwrap_or_default();
            let (stem, parsed_method) = split_method_suffix(stem);
            if let Some(parsed_method) = parsed_method {
                method = parsed_method;
            }

            if stem != "index" {
                segments.push(route_segment_from_file_name(stem)?);
            }
        } else {
            segments.push(route_segment_from_file_name(component)?);
        }
    }

    let path = if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    };

    Ok(EmbeddedRoute {
        method: method.as_str().to_string(),
        path,
        source_path: source_path.to_path_buf(),
        source_kind,
    })
}

pub fn match_route<'a>(
    app: &'a EmbeddedAppDefinition,
    method: &str,
    path: &str,
) -> Option<EmbeddedRouteMatch<'a>> {
    let wanted_method = method.to_uppercase();
    let normalized_path = normalize_route_path(path);

    for route in &app.routes {
        if route.method != wanted_method {
            continue;
        }

        if let Some(params) = match_path_pattern(&route.path, &normalized_path) {
            return Some(EmbeddedRouteMatch { route, params });
        }
    }

    None
}

pub fn validate_embedded_app(app: &EmbeddedAppDefinition) -> Result<()> {
    if app.routes.is_empty() {
        bail!("Embedded app must register at least one route");
    }

    for route in &app.routes {
        if route.path.trim().is_empty() {
            bail!(
                "Embedded route sourced from '{}' is missing a path",
                route.source_path.display()
            );
        }
    }

    Ok(())
}

fn split_method_suffix(stem: &str) -> (&str, Option<EmbeddedHttpMethod>) {
    if let Some((base, suffix)) = stem.rsplit_once('.') {
        if let Some(method) = EmbeddedHttpMethod::from_suffix(suffix) {
            return (base, Some(method));
        }
    }

    (stem, None)
}

fn route_segment_from_file_name(segment: &str) -> Result<String> {
    if segment.is_empty() || segment == "." {
        return Ok(String::new());
    }

    if let Some(name) = segment.strip_prefix('[').and_then(|value| value.strip_suffix(']')) {
        if name.is_empty() {
            bail!("Empty route placeholder is not allowed");
        }
        return Ok(format!(":{}", name));
    }

    Ok(segment.to_string())
}

fn match_path_pattern(pattern: &str, path: &str) -> Option<HashMap<String, String>> {
    let pattern_segments: Vec<_> = pattern.trim_matches('/').split('/').collect();
    let path_segments: Vec<_> = path.trim_matches('/').split('/').collect();

    let pattern_segments = if pattern_segments.len() == 1 && pattern_segments[0].is_empty() {
        Vec::new()
    } else {
        pattern_segments
    };

    let path_segments = if path_segments.len() == 1 && path_segments[0].is_empty() {
        Vec::new()
    } else {
        path_segments
    };

    if pattern_segments.len() != path_segments.len() {
        return None;
    }

    let mut params = HashMap::new();
    for (pattern_segment, path_segment) in pattern_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = pattern_segment.strip_prefix(':') {
            params.insert(name.to_string(), (*path_segment).to_string());
        } else if pattern_segment != path_segment {
            return None;
        }
    }

    Some(params)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_route_paths_without_duplicate_slashes() {
        assert_eq!(join_route_paths("/api/", "/status"), "/api/status");
    }

    #[test]
    fn matches_named_route_params() {
        let app = EmbeddedAppDefinition {
            listen: EmbeddedListenConfig::default(),
            routes: vec![EmbeddedRoute {
                method: "GET".to_string(),
                path: "/printer/:id".to_string(),
                source_path: PathBuf::from("app/printer/[id].bxm"),
                source_kind: EmbeddedSourceKind::Template,
            }],
        };

        let matched = match_route(&app, "GET", "/printer/km-01").unwrap();
        assert_eq!(matched.route.path, "/printer/:id");
        assert_eq!(matched.params.get("id"), Some(&"km-01".to_string()));
    }

    #[test]
    fn builds_get_route_from_template_file() {
        let route = route_from_app_file(
            Path::new("/project/app"),
            Path::new("/project/app/printer/[id].bxm"),
        )
        .unwrap();

        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/printer/:id");
        assert_eq!(route.source_kind, EmbeddedSourceKind::Template);
    }

    #[test]
    fn builds_post_route_from_script_file_suffix() {
        let route = route_from_app_file(
            Path::new("/project/app"),
            Path::new("/project/app/print.post.bxs"),
        )
        .unwrap();

        assert_eq!(route.method, "POST");
        assert_eq!(route.path, "/print");
        assert_eq!(route.source_kind, EmbeddedSourceKind::Script);
    }

    #[test]
    fn index_template_maps_to_directory_root() {
        let route = route_from_app_file(
            Path::new("/project/app"),
            Path::new("/project/app/admin/index.bxm"),
        )
        .unwrap();

        assert_eq!(route.method, "GET");
        assert_eq!(route.path, "/admin");
    }

    #[test]
    fn request_route_params_override_query_params_in_url_scope() {
        let mut route_params = HashMap::new();
        route_params.insert("id".to_string(), "printer-2".to_string());

        let mut query = HashMap::new();
        query.insert("id".to_string(), "query-value".to_string());
        query.insert("page".to_string(), "1".to_string());

        let request = EmbeddedRequest::with_route_and_query("GET", "/printer/printer-2", route_params, query);
        assert_eq!(request.url.get("id"), Some(&"printer-2".to_string()));
        assert_eq!(request.url.get("page"), Some(&"1".to_string()));
    }
}
