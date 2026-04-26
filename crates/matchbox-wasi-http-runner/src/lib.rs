use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::sync::OnceLock;

use wasi::http::types::{
    Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

#[path = "../../matchbox-server/src/webroot_core.rs"]
mod webroot_core;

use webroot_core::{
    EmbeddedAssetPackage, EmbeddedAssetStore, WebrootConfig, WebrootEngine, WebrootRequest,
    WebrootResponse,
};

wasi::http::proxy::export!(MatchBoxWasiHttp);

const EMBED_MAGIC: [u8; 8] = [b'M', b'B', b'W', b'H', 0, 0, 0, 1];
const EMBED_CAPACITY: usize = 4 * 1024 * 1024;
const EMBED_DATA_CAPACITY: usize = EMBED_CAPACITY - 12;

#[used]
#[unsafe(no_mangle)]
static mut MATCHBOX_WASI_HTTP_EMBED: [u8; EMBED_CAPACITY] = empty_embed_region();

static ENGINE: OnceLock<Result<WebrootEngine<EmbeddedAssetStore>, String>> = OnceLock::new();

struct MatchBoxWasiHttp;

impl wasi::exports::http::incoming_handler::Guest for MatchBoxWasiHttp {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let response = match request_to_webroot_request(&request).and_then(handle_webroot_request) {
            Ok(response) => response,
            Err(error) => WebrootResponse {
                status: 500,
                headers: HashMap::from([("content-type".to_string(), "text/plain".to_string())]),
                body: error.into_bytes(),
            },
        };
        send_response(response_out, response);
    }
}

const fn empty_embed_region() -> [u8; EMBED_CAPACITY] {
    let mut bytes = [0xA5u8; EMBED_CAPACITY];
    let mut index = 0;
    while index < EMBED_MAGIC.len() {
        bytes[index] = EMBED_MAGIC[index];
        index += 1;
    }
    bytes[8] = 0;
    bytes[9] = 0;
    bytes[10] = 0;
    bytes[11] = 0;
    bytes
}

fn handle_webroot_request(request: WebrootRequest) -> Result<WebrootResponse, String> {
    match engine() {
        Ok(engine) => engine.handle(request).map_err(|err| err.to_string()),
        Err(error) => Err(error.clone()),
    }
}

fn engine() -> Result<&'static WebrootEngine<EmbeddedAssetStore>, &'static String> {
    match ENGINE.get_or_init(load_engine) {
        Ok(engine) => Ok(engine),
        Err(error) => Err(error),
    }
}

fn load_engine() -> Result<WebrootEngine<EmbeddedAssetStore>, String> {
    let payload = embedded_payload()?;
    let package: WasiHttpPayload = bincode::deserialize(payload).map_err(|err| {
        format!(
            "failed to decode embedded webroot payload: {err:?}; first bytes: {:02x?}",
            &payload[..payload.len().min(16)]
        )
    })?;
    let store = EmbeddedAssetStore::from_package(package.assets);
    Ok(WebrootEngine::new(store, package.config))
}

fn embedded_payload() -> Result<&'static [u8], String> {
    let embed_ptr = core::ptr::addr_of!(MATCHBOX_WASI_HTTP_EMBED).cast::<u8>();
    for (index, expected) in EMBED_MAGIC.iter().enumerate() {
        let actual = unsafe { embed_ptr.add(index).read_volatile() };
        if actual != *expected {
            return Err("WASI HTTP runner payload sentinel is missing".to_string());
        }
    }
    let len_bytes = [
        unsafe { embed_ptr.add(8).read_volatile() },
        unsafe { embed_ptr.add(9).read_volatile() },
        unsafe { embed_ptr.add(10).read_volatile() },
        unsafe { embed_ptr.add(11).read_volatile() },
    ];
    let len = u32::from_le_bytes(len_bytes) as usize;
    if len == 0 {
        return Err("WASI HTTP runner has no embedded webroot payload".to_string());
    }
    if len > EMBED_DATA_CAPACITY {
        return Err("WASI HTTP runner embedded webroot payload is too large".to_string());
    }
    Ok(unsafe { core::slice::from_raw_parts(embed_ptr.add(12), len) })
}

#[derive(serde::Deserialize, serde::Serialize)]
struct WasiHttpPayload {
    config: WebrootConfig,
    assets: EmbeddedAssetPackage,
}

fn request_to_webroot_request(request: &IncomingRequest) -> Result<WebrootRequest, String> {
    let method = method_to_string(request.method());
    let path_with_query = request.path_with_query().unwrap_or_else(|| "/".to_string());
    let (path, raw_query) = split_path_query(&path_with_query);
    let headers = headers_to_map(request.headers());
    let query = raw_query
        .map(|query| {
            url::form_urlencoded::parse(query.as_bytes())
                .into_owned()
                .collect::<HashMap<String, String>>()
        })
        .unwrap_or_default();
    let body = read_body(request)?;

    Ok(WebrootRequest {
        method,
        path,
        query,
        form: HashMap::new(),
        headers,
        body,
    })
}

fn method_to_string(method: Method) -> String {
    match method {
        Method::Get => "GET",
        Method::Head => "HEAD",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
        Method::Connect => "CONNECT",
        Method::Options => "OPTIONS",
        Method::Trace => "TRACE",
        Method::Patch => "PATCH",
        Method::Other(value) => return value,
    }
    .to_string()
}

fn split_path_query(path_with_query: &str) -> (String, Option<String>) {
    match path_with_query.split_once('?') {
        Some((path, query)) => (path.to_string(), Some(query.to_string())),
        None => (path_with_query.to_string(), None),
    }
}

fn headers_to_map(headers: Fields) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for (name, value) in headers.entries() {
        if let Ok(value) = String::from_utf8(value) {
            map.insert(name.to_ascii_lowercase(), value);
        }
    }
    map
}

fn read_body(request: &IncomingRequest) -> Result<Vec<u8>, String> {
    let Ok(body) = request.consume() else {
        return Ok(Vec::new());
    };
    let mut bytes = Vec::new();
    {
        let mut stream = body
            .stream()
            .map_err(|_| "failed to read request body".to_string())?;
        stream
            .read_to_end(&mut bytes)
            .map_err(|err| err.to_string())?;
    }
    let _trailers = wasi::http::types::IncomingBody::finish(body);
    Ok(bytes)
}

fn send_response(response_out: ResponseOutparam, response: WebrootResponse) {
    let headers = Fields::new();
    for (name, value) in response.headers {
        let _ = headers.append(&name, value.as_bytes());
    }
    let resp = OutgoingResponse::new(headers);
    let _ = resp.set_status_code(response.status);
    let body = resp.body().unwrap();

    ResponseOutparam::set(response_out, Ok(resp));

    let mut out = body.write().unwrap();
    let _ = out.write_all(&response.body);
    let _ = out.flush();
    drop(out);

    let _ = OutgoingBody::finish(body, None);
}
