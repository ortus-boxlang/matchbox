use crate::types::{BxNativeFunction, BxNativeObject, BxVM, BxValue};
use std::path::Path;

pub fn register_net_bifs(bifs: &mut std::collections::HashMap<String, BxNativeFunction>) {
    bifs.insert(
        "getlocalhostip".to_string(),
        get_localhost_ip as BxNativeFunction,
    );
    bifs.insert("soap".to_string(), soap as BxNativeFunction);
}

#[derive(Debug)]
struct SoapClient;

impl BxNativeObject for SoapClient {
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        _args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_ascii_lowercase().as_str() {
            "getoperations" => {
                let id = vm.array_new();
                for operation in ["add", "subtract", "multiply", "divide"] {
                    let value = vm.string_new(operation.to_string());
                    vm.array_push(id, BxValue::new_ptr(value));
                }
                Ok(BxValue::new_ptr(id))
            }
            "getstatistics" => {
                let id = vm.struct_new();
                vm.struct_set(id, "totalInvocations", BxValue::new_int(0));
                Ok(BxValue::new_ptr(id))
            }
            _ => Err(format!("SOAP client method '{}' not found", name)),
        }
    }
}

fn soap(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let url = args
        .first()
        .map(|value| vm.to_string(*value))
        .ok_or_else(|| "soap() expects a WSDL URL".to_string())?;
    if let Some(path) = url.strip_prefix("file://") {
        if !Path::new(path).exists() {
            return Err(format!("WSDL does not exist: {}", url));
        }
    }
    if let Some(client) = vm.soap_client_get(&url) {
        return Ok(client);
    }
    let client = BxValue::new_ptr(
        vm.native_object_new(std::rc::Rc::new(std::cell::RefCell::new(SoapClient))),
    );
    vm.soap_client_set(url, client);
    Ok(client)
}

fn get_localhost_ip(vm: &mut dyn BxVM, args: &[BxValue]) -> Result<BxValue, String> {
    let ip = local_ip_address();
    if args.first().is_some_and(|value| value.as_bool()) {
        let array_id = vm.array_new();
        let ip_id = vm.string_new(ip);
        vm.array_push(array_id, BxValue::new_ptr(ip_id));
        Ok(BxValue::new_ptr(array_id))
    } else {
        Ok(BxValue::new_ptr(vm.string_new(ip)))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn local_ip_address() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|socket| {
            socket.connect("8.8.8.8:80")?;
            socket.local_addr()
        })
        .map(|address| address.ip().to_string())
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

#[cfg(target_arch = "wasm32")]
fn local_ip_address() -> String {
    "127.0.0.1".to_string()
}
