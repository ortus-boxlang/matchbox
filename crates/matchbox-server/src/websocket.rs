use axum::{
    extract::{
        ws::{Message as WebSocketMessage, WebSocket, WebSocketUpgrade},
        State,
    },
    http::{HeaderMap, Method},
    response::IntoResponse,
};
use matchbox_vm::{
    types::{BxNativeObject, BxVM, BxValue},
    vm::VM,
};
use serde_json::Value as JsonValue;
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::mpsc::{Receiver as StdReceiver, Sender as StdSender},
    sync::{Arc},
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use uuid::Uuid;
use crate::{RequestData, request_data_from_parts, bx_to_json};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketConfig {
    pub uri: String,
    pub listener_class: String,
    pub listener_state: JsonValue,
    #[serde(default = "default_handler")]
    pub handler: String,
}

fn default_handler() -> String {
    "WebSocket.bx".to_string()
}

#[derive(Clone)]
pub struct WebSocketRuntimeHandle {
    pub uri: String,
    pub commands: StdSender<WebSocketRuntimeCommand>,
}

pub enum WebSocketRuntimeCommand {
    Connect {
        connection_id: String,
        request: RequestData,
        outbound: UnboundedSender<WebSocketOutbound>,
    },
    Message {
        connection_id: String,
        message: IncomingWebSocketMessage,
    },
    Close {
        connection_id: String,
    },
}

pub enum IncomingWebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug)]
pub enum WebSocketOutbound {
    Text(String),
    Binary(Vec<u8>),
    Close {
        code: u16,
        reason: String,
    },
}

#[derive(Debug)]
pub struct WebSocketChannelObject {
    pub connection_id: String,
    pub request: RequestData,
    pub outbound: Rc<RefCell<HashMap<String, UnboundedSender<WebSocketOutbound>>>>,
}

impl BxNativeObject for WebSocketChannelObject {
    fn get_property(&self, name: &str) -> BxValue {
        match name.to_lowercase().as_str() {
            "id" => BxValue::new_null(),
            _ => BxValue::new_null(),
        }
    }

    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        _id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        match name.to_lowercase().as_str() {
            "sendmessage" | "sendtext" => {
                if args.is_empty() {
                    return Err("sendMessage() requires a message".to_string());
                }
                let message = vm.to_string(args[0]);
                if let Some(sender) = self.outbound.borrow().get(&self.connection_id) {
                    sender
                        .send(WebSocketOutbound::Text(message))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                } else {
                    return Err("WebSocket connection is closed".to_string());
                }
                Ok(BxValue::new_null())
            }
            "broadcastmessage" | "broadcasttext" => {
                if args.is_empty() {
                    return Err("broadcastMessage() requires a message".to_string());
                }
                let message = vm.to_string(args[0]);
                for sender in self.outbound.borrow().values() {
                    sender
                        .send(WebSocketOutbound::Text(message.clone()))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                }
                Ok(BxValue::new_null())
            }
            "sendjson" => {
                if args.is_empty() {
                    return Err("sendJson() requires a payload".to_string());
                }
                let payload = bx_to_json(vm, args[0])?;
                let text = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
                if let Some(sender) = self.outbound.borrow().get(&self.connection_id) {
                    sender
                        .send(WebSocketOutbound::Text(text))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                } else {
                    return Err("WebSocket connection is closed".to_string());
                }
                Ok(BxValue::new_null())
            }
            "broadcastjson" => {
                if args.is_empty() {
                    return Err("broadcastJson() requires a payload".to_string());
                }
                let payload = bx_to_json(vm, args[0])?;
                let text = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
                for sender in self.outbound.borrow().values() {
                    sender
                        .send(WebSocketOutbound::Text(text.clone()))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                }
                Ok(BxValue::new_null())
            }
            "sendbytes" => {
                if args.is_empty() {
                    return Err("sendBytes() requires a bytes payload".to_string());
                }
                let payload = vm.to_bytes(args[0])?;
                if let Some(sender) = self.outbound.borrow().get(&self.connection_id) {
                    sender
                        .send(WebSocketOutbound::Binary(payload))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                } else {
                    return Err("WebSocket connection is closed".to_string());
                }
                Ok(BxValue::new_null())
            }
            "broadcastbytes" => {
                if args.is_empty() {
                    return Err("broadcastBytes() requires a bytes payload".to_string());
                }
                let payload = vm.to_bytes(args[0])?;
                for sender in self.outbound.borrow().values() {
                    sender
                        .send(WebSocketOutbound::Binary(payload.clone()))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                }
                Ok(BxValue::new_null())
            }
            "close" => {
                let code = args
                    .first()
                    .map(|value| vm.to_string(*value).parse::<u16>().unwrap_or(1000))
                    .unwrap_or(1000);
                let reason = args.get(1).map(|value| vm.to_string(*value)).unwrap_or_default();
                if let Some(sender) = self.outbound.borrow().get(&self.connection_id) {
                    sender
                        .send(WebSocketOutbound::Close { code, reason })
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                } else {
                    return Err("WebSocket connection is closed".to_string());
                }
                Ok(BxValue::new_null())
            }
            "getid" => Ok(BxValue::new_ptr(vm.string_new(self.connection_id.clone()))),
            "getpath" => Ok(BxValue::new_ptr(vm.string_new(self.request.path.clone()))),
            "geturl" => Ok(BxValue::new_ptr(vm.string_new(self.request.full_url.clone()))),
            "gethttpheader" => {
                if args.is_empty() {
                    return Err("getHTTPHeader() requires a header name".to_string());
                }
                let key = vm.to_string(args[0]).to_lowercase();
                if let Some(value) = self.request.headers.get(&key) {
                    Ok(BxValue::new_ptr(vm.string_new(value.clone())))
                } else if let Some(default) = args.get(1) {
                    Ok(*default)
                } else {
                    Ok(BxValue::new_null())
                }
            }
            _ => Err(format!("Method {} not found on websocket channel.", name)),
        }
    }
}

pub async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(runtime): State<Arc<WebSocketRuntimeHandle>>,
    method: Method,
    headers: HeaderMap,
    uri: axum::http::Uri,
) -> impl IntoResponse {
    let request = request_data_from_parts(method, uri.path(), uri.query(), &headers, Vec::new());
    let connection_id = Uuid::new_v4().to_string();

    ws.on_upgrade(move |socket| websocket_connection_loop(socket, runtime, connection_id, request))
}

pub async fn websocket_connection_loop(
    mut socket: WebSocket,
    runtime: Arc<WebSocketRuntimeHandle>,
    connection_id: String,
    request: RequestData,
) {
    let (outbound_tx, mut outbound_rx): (
        UnboundedSender<WebSocketOutbound>,
        UnboundedReceiver<WebSocketOutbound>,
    ) = unbounded_channel();

    if runtime
        .commands
        .send(WebSocketRuntimeCommand::Connect {
            connection_id: connection_id.clone(),
            request,
            outbound: outbound_tx,
        })
        .is_err()
    {
        let _ = socket.close().await;
        return;
    }

    loop {
        tokio::select! {
            outbound = outbound_rx.recv() => {
                let Some(outbound) = outbound else {
                    break;
                };
                let send_result = match outbound {
                    WebSocketOutbound::Text(text) => socket.send(WebSocketMessage::Text(text)).await,
                    WebSocketOutbound::Binary(bytes) => socket.send(WebSocketMessage::Binary(bytes)).await,
                    WebSocketOutbound::Close { code, reason } => {
                        socket
                            .send(WebSocketMessage::Close(Some(axum::extract::ws::CloseFrame {
                                code,
                                reason: reason.into(),
                            })))
                            .await
                    }
                };
                if send_result.is_err() {
                    break;
                }
            }
            inbound = socket.recv() => {
                let Some(inbound) = inbound else {
                    break;
                };
                match inbound {
                    Ok(WebSocketMessage::Text(text)) => {
                        if runtime.commands.send(WebSocketRuntimeCommand::Message {
                            connection_id: connection_id.clone(),
                            message: IncomingWebSocketMessage::Text(text),
                        }).is_err() {
                            break;
                        }
                    }
                    Ok(WebSocketMessage::Binary(bytes)) => {
                        if runtime.commands.send(WebSocketRuntimeCommand::Message {
                            connection_id: connection_id.clone(),
                            message: IncomingWebSocketMessage::Binary(bytes),
                        }).is_err() {
                            break;
                        }
                    }
                    Ok(WebSocketMessage::Close(_)) => break,
                    Ok(WebSocketMessage::Ping(payload)) => {
                        if socket.send(WebSocketMessage::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Ok(WebSocketMessage::Pong(_)) => {}
                    Err(_) => break,
                }
            }
        }
    }

    let _ = runtime
        .commands
        .send(WebSocketRuntimeCommand::Close { connection_id });
}

use std::path::PathBuf;
use std::sync::Mutex;
use crate::app_server::{BuildState, install_web_namespace};

pub fn websocket_runtime_main(
    chunk: matchbox_vm::vm::chunk::Chunk,
    config: WebSocketConfig,
    commands: StdReceiver<WebSocketRuntimeCommand>,
    web_context: Option<(Arc<Mutex<BuildState>>, PathBuf)>,
) -> anyhow::Result<()> {
    let mut vm = VM::new();
    if let Some((build_state, app_root)) = web_context {
        install_web_namespace(&mut vm, build_state, app_root);
    }
    vm.interpret(chunk)?;

    let listener = vm.instantiate_global_class_without_constructor(&config.listener_class)?;
    vm.set_instance_variables_json(listener, config.listener_state.clone())?;
    vm.insert_global("__websocketlistener".to_string(), listener);
    let channel_registry_id = vm.struct_new();
    vm.insert_global(
        "__websocketconnections".to_string(),
        BxValue::new_ptr(channel_registry_id),
    );

    let outbound_senders: Rc<RefCell<HashMap<String, UnboundedSender<WebSocketOutbound>>>> =
        Rc::new(RefCell::new(HashMap::new()));

    while let Ok(command) = commands.recv() {
        match command {
            WebSocketRuntimeCommand::Connect {
                connection_id,
                request,
                outbound,
            } => {
                outbound_senders
                    .borrow_mut()
                    .insert(connection_id.clone(), outbound);
                let channel = build_websocket_channel(
                    &mut vm,
                    &connection_id,
                    request,
                    outbound_senders.clone(),
                );
                vm.struct_set(channel_registry_id, &connection_id, channel);
                if let Err(err) = vm.call_method_value(listener, "onconnect", vec![channel]) {
                    eprintln!("WebSocket onConnect error: {}", err);
                    send_websocket_close(outbound_senders.clone(), &connection_id, 1011, "Internal error");
                }
            }
            WebSocketRuntimeCommand::Message {
                connection_id,
                message,
            } => {
                let channel = vm.struct_get(channel_registry_id, &connection_id);
                if channel.is_null() {
                    continue;
                }
                let message_value = match message {
                    IncomingWebSocketMessage::Text(text) => BxValue::new_ptr(vm.string_new(text)),
                    IncomingWebSocketMessage::Binary(bytes) => BxValue::new_ptr(vm.bytes_new(bytes)),
                };
                if let Err(err) = vm.call_method_value(listener, "onmessage", vec![message_value, channel]) {
                    eprintln!("WebSocket onMessage error: {}", err);
                    send_websocket_close(outbound_senders.clone(), &connection_id, 1011, "Internal error");
                }
            }
            WebSocketRuntimeCommand::Close { connection_id } => {
                let channel = vm.struct_get(channel_registry_id, &connection_id);
                if !channel.is_null() {
                    let _ = vm.call_method_value(listener, "onclose", vec![channel]);
                    let _ = vm.struct_delete(channel_registry_id, &connection_id);
                }
                outbound_senders.borrow_mut().remove(&connection_id);
            }
        }
    }

    Ok(())
}

fn build_websocket_channel(
    vm: &mut VM,
    connection_id: &str,
    request: RequestData,
    outbound: Rc<RefCell<HashMap<String, UnboundedSender<WebSocketOutbound>>>>,
) -> BxValue {
    let id = vm.native_object_new(Rc::new(RefCell::new(WebSocketChannelObject {
        connection_id: connection_id.to_string(),
        request,
        outbound,
    })));
    BxValue::new_ptr(id)
}

fn send_websocket_close(
    outbound: Rc<RefCell<HashMap<String, UnboundedSender<WebSocketOutbound>>>>,
    connection_id: &str,
    code: u16,
    reason: &str,
) {
    if let Some(sender) = outbound.borrow().get(connection_id) {
        let _ = sender.send(WebSocketOutbound::Close {
            code,
            reason: reason.to_string(),
        });
    }
}
