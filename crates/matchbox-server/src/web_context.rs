//! BoxLang web operations belong to the shared runtime, not individual HTTP hosts.
use crate::webroot_core::{WebrootContext, WebrootRequest, WebrootResponse};
use http::header::{HeaderName, HeaderValue};
use matchbox_vm::{
    types::{BxNativeObject, BxVM, BxValue, NativeCallback},
    vm::VM,
};
use std::{cell::RefCell, rc::Rc};

type ResponseState = Rc<RefCell<WebrootResponse>>;

#[derive(Clone, Debug)]
struct ActiveWebContext {
    request: Rc<WebrootRequest>,
    response: ResponseState,
    protocol: String,
}
impl BxNativeObject for ActiveWebContext {
    fn native_data(&self, kind: &str) -> Option<*mut std::ffi::c_void> {
        (kind == "matchbox.web_context.v1").then_some(self as *const Self as *mut std::ffi::c_void)
    }
    fn external_allocation_bytes(&self) -> usize {
        self.request.body.capacity()
    }
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }
    fn set_property(&mut self, _name: &str, _value: BxValue) {}
    fn call_method(
        &mut self,
        _vm: &mut dyn BxVM,
        _id: usize,
        _name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        Err("Web context is opaque".into())
    }
}
fn active_context(vm: &dyn BxVM) -> Result<ActiveWebContext, String> {
    let value = vm
        .get_global_value("__mb_web_context")
        .ok_or("Web operation requires an active request")?;
    let pointer = vm
        .native_object_data(value, "matchbox.web_context.v1")
        .ok_or("Invalid web request context")?;
    // The private brand identifies this VM-owned object; clone before any VM allocation or callback.
    Ok(unsafe { (&*(pointer as *const ActiveWebContext)).clone() })
}

pub(crate) fn install_web_context(
    vm: &mut VM,
    request: Rc<WebrootRequest>,
    context: &WebrootContext,
) -> anyhow::Result<ResponseState> {
    vm.register_component_runtime()?;
    let response = Rc::new(RefCell::new(WebrootResponse {
        status: 200,
        headers: vec![("content-type".into(), "text/html".into())],
        body: Vec::new(),
    }));
    let value = vm.native_object_new(Rc::new(RefCell::new(ActiveWebContext {
        request,
        response: response.clone(),
        protocol: context.protocol.clone(),
    })));
    vm.insert_global("__mb_web_context".into(), BxValue::new_ptr(value));
    vm.register_callback(
        "__mb_component",
        NativeCallback {
            parameters: vec!["name".into(), "attributes".into(), "body".into()],
            callback: Rc::new(|vm, args| {
                let state = active_context(vm)?.response;
                if args.len() != 3 || !vm.is_struct_value(args[1]) {
                    return Err("Component requires a name, attribute struct and body".into());
                }
                let name = vm.to_string(args[0]).to_ascii_lowercase();
                let attributes = args[1].as_gc_id().unwrap();
                let body = args[2];
                match name.as_str() {
                    "header" => {
                        if !body.is_null() {
                            return Err("header does not allow a body".into());
                        }
                        validate_attributes(
                            vm,
                            attributes,
                            &["name", "value", "charset", "statuscode", "statustext"],
                        )?;
                        let code = vm.struct_get(attributes, "statusCode");
                        if !code.is_null() {
                            let code = vm
                                .to_string(code)
                                .parse::<u16>()
                                .map_err(|_| "Invalid HTTP statusCode")?;
                            if !(100..=599).contains(&code) {
                                return Err("statusCode must be between 100 and 599".into());
                            }
                            state.borrow_mut().status = code;
                        } else {
                            let name = vm.struct_get(attributes, "name");
                            let value = vm.struct_get(attributes, "value");
                            let name = if name.is_null() {
                                String::new()
                            } else {
                                vm.to_string(name)
                            };
                            let value = if value.is_null() {
                                String::new()
                            } else {
                                vm.to_string(value)
                            };
                            append_header(&mut state.borrow_mut(), &name, &value)?;
                        }
                    }
                    "content" => {
                        validate_attributes(vm, attributes, &["type", "reset"])?;
                        let content_type = vm.struct_get(attributes, "type");
                        if !content_type.is_null() {
                            let content_type = vm.to_string(content_type);
                            if content_type.trim().is_empty() {
                                return Err("content type must not be empty".into());
                            }
                            let mut response = state.borrow_mut();
                            append_header(&mut response, "content-type", &content_type)?;
                            // Validate before replacing, so a caught error leaves headers intact.
                            let last = response.headers.pop().unwrap();
                            response.headers.retain(|(name, _)| name != "content-type");
                            response.headers.push(last);
                        }
                        let reset = vm.struct_get(attributes, "reset");
                        if boolean(vm, reset, true)? {
                            vm.end_output_capture();
                            vm.begin_output_capture();
                        }
                        if !body.is_null() {
                            let chunk = vm
                                .current_chunk()
                                .ok_or("Web component context unavailable")?;
                            let result = vm.call_function_by_value(&body, vec![], chunk)?;
                            if !result.is_null() {
                                return Ok(result);
                            }
                        }
                    }
                    _ => return vm.invoke_component(&name, args[1], body),
                }
                let callback = vm
                    .get_global_value("__mb_body_result")
                    .ok_or("Component body result unavailable")?;
                let chunk = vm
                    .current_chunk()
                    .ok_or("Web component context unavailable")?;
                vm.call_function_by_value(
                    &callback,
                    vec![BxValue::new_number(0.0), BxValue::new_null()],
                    chunk,
                )
            }),
        },
    )
    .map_err(anyhow::Error::msg)?;
    vm.register_callback(
        "getHTTPRequestData",
        NativeCallback {
            parameters: vec!["includeBody".into()],
            callback: Rc::new(|vm, args| {
                let context = active_context(vm)?;
                let request = &context.request;
                let protocol = &context.protocol;
                if args.len() > 1 {
                    return Err("getHTTPRequestData accepts only includeBody".into());
                }
                let include_body = boolean(
                    vm,
                    args.first().copied().unwrap_or(BxValue::new_null()),
                    true,
                )?;
                // Root the result during allocations; callbacks can trigger collection.
                let result = vm.struct_new();
                vm.push_root(BxValue::new_ptr(result));
                let headers = vm.struct_new();
                vm.struct_set(result, "headers", BxValue::new_ptr(headers));
                for (name, value) in &request.headers {
                    let value = BxValue::new_ptr(vm.string_new(value.clone()));
                    vm.struct_set(headers, name, value);
                }
                for (key, value) in [
                    ("method", request.method.to_ascii_uppercase()),
                    ("protocol", protocol.clone()),
                ] {
                    let value = BxValue::new_ptr(vm.string_new(value));
                    vm.struct_set(result, key, value);
                }
                if include_body {
                    let content_type = request
                        .headers
                        .get("content-type")
                        .map(String::as_str)
                        .unwrap_or("");
                    let chunked = request
                        .headers
                        .get("transfer-encoding")
                        .is_some_and(|value| {
                            value
                                .split(',')
                                .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
                        });
                    let no_body = request.body.is_empty()
                        && (!chunked || !request.headers.contains_key("content-type"));
                    let content = if text_content_type(content_type) || no_body {
                        BxValue::new_ptr(
                            vm.string_new(String::from_utf8_lossy(&request.body).into_owned()),
                        )
                    } else {
                        BxValue::new_ptr(vm.bytes_new(request.body.clone()))
                    };
                    vm.struct_set(result, "content", content);
                }
                vm.pop_root();
                Ok(BxValue::new_ptr(result))
            }),
        },
    )
    .map_err(anyhow::Error::msg)?;
    Ok(response)
}

fn boolean(vm: &mut dyn BxVM, value: BxValue, default: bool) -> Result<bool, String> {
    if value.is_null() {
        return Ok(default);
    }
    vm.cast_value_to_type(value, "boolean")
        .map(|value| value.as_bool())
}

fn validate_attributes(vm: &dyn BxVM, attributes: usize, allowed: &[&str]) -> Result<(), String> {
    for key in vm.struct_key_array(attributes) {
        if !allowed.iter().any(|name| key.eq_ignore_ascii_case(name)) {
            return Err(format!("Unsupported web component attribute: {key}"));
        }
    }
    Ok(())
}

fn append_header(response: &mut WebrootResponse, name: &str, value: &str) -> Result<(), String> {
    let name =
        HeaderName::from_bytes(name.as_bytes()).map_err(|_| "Invalid HTTP response header name")?;
    if matches!(
        name.as_str(),
        "content-length"
            | "transfer-encoding"
            | "connection"
            | "trailer"
            | "upgrade"
            | "keep-alive"
            | "proxy-connection"
    ) {
        return Err("HTTP framing and connection headers are controlled by the host".into());
    }
    HeaderValue::from_str(value).map_err(|_| "Invalid HTTP response header value")?;
    response
        .headers
        .push((name.as_str().to_string(), value.to_string()));
    Ok(())
}

fn text_content_type(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    mime.starts_with("text/")
        || (mime.starts_with("application/") && (mime.ends_with("json") || mime.ends_with("xml")))
        || matches!(
            mime.as_str(),
            "application/x-www-form-urlencoded"
                | "application/javascript"
                | "application/graphql"
                | "application/yaml"
                | "application/x-yaml"
                | "application/x-ndjson"
                | "application/csv"
                | "application/sql"
                | "application/rtf"
        )
}
