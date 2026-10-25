use crate::app_server::{ResponseData, response_to_http};
use crate::{RequestData, bx_to_json, request_data_from_parts};
use axum::{
    extract::{
        State,
        ws::{Message as WebSocketMessage, WebSocket, WebSocketUpgrade},
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
    cell::RefCell, collections::HashMap, rc::Rc, sync::Arc, sync::mpsc::Receiver as StdReceiver,
};
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{Receiver, Sender, channel};
use uuid::Uuid;

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
    pub commands: Arc<dyn Fn(WebSocketRuntimeCommand) -> Result<(), String> + Send + Sync>,
    pub authorize: Option<Arc<dyn Fn(RequestData) -> Result<ResponseData, String> + Send + Sync>>,
    pub connections: Arc<Semaphore>,
    pub shutdown: tokio::sync::watch::Sender<bool>,
}

impl WebSocketRuntimeHandle {
    pub fn new(
        uri: String,
        commands: impl Fn(WebSocketRuntimeCommand) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            uri,
            commands: Arc::new(commands),
            authorize: None,
            connections: Arc::new(Semaphore::new(16)),
            shutdown: tokio::sync::watch::channel(false).0,
        }
    }
    async fn dispatch(&self, command: WebSocketRuntimeCommand) -> Result<(), String> {
        let sender = self.commands.clone();
        tokio::task::spawn_blocking(move || sender(command))
            .await
            .map_err(|_| "WebSocket dispatcher stopped".to_string())?
    }
}

pub enum WebSocketRuntimeCommand {
    Connect {
        connection_id: String,
        request: RequestData,
        outbound: Sender<WebSocketOutbound>,
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
    Close { code: u16, reason: String },
}
#[cfg(test)]
mod limits {
    use super::*;
    #[test]
    fn outbound_messages_and_queue_are_bounded() {
        let (sender, mut receiver) = channel(1);
        assert!(
            sender
                .send_bounded(WebSocketOutbound::Text("x".repeat(64 * 1024 + 1)))
                .is_err()
        );
        assert!(receiver.try_recv().is_err());
        sender
            .send_bounded(WebSocketOutbound::Text("first".into()))
            .unwrap();
        assert!(
            sender
                .send_bounded(WebSocketOutbound::Text("second".into()))
                .is_err()
        );
        assert!(
            matches!(receiver.try_recv(), Ok(WebSocketOutbound::Text(text)) if text == "first")
        );
    }
}

trait BoundedSend {
    fn send_bounded(&self, message: WebSocketOutbound) -> Result<(), String>;
}
impl BoundedSend for Sender<WebSocketOutbound> {
    fn send_bounded(&self, message: WebSocketOutbound) -> Result<(), String> {
        let size = match &message {
            WebSocketOutbound::Text(text) => text.len(),
            WebSocketOutbound::Binary(bytes) => bytes.len(),
            WebSocketOutbound::Close { reason, .. } => reason.len(),
        };
        if size > 64 * 1024 {
            return Err("WebSocket outbound message exceeds limit".into());
        }
        self.try_send(message)
            .map_err(|_| "WebSocket outbound queue is full or closed".to_string())
    }
}

#[derive(Debug)]
pub struct WebSocketChannelObject {
    pub connection_id: String,
    pub request: RequestData,
    pub outbound: Rc<RefCell<HashMap<String, Sender<WebSocketOutbound>>>>,
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
                        .send_bounded(WebSocketOutbound::Text(message))
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
                        .send_bounded(WebSocketOutbound::Text(message.clone()))
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
                        .send_bounded(WebSocketOutbound::Text(text))
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
                        .send_bounded(WebSocketOutbound::Text(text.clone()))
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
                        .send_bounded(WebSocketOutbound::Binary(payload))
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
                        .send_bounded(WebSocketOutbound::Binary(payload.clone()))
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                }
                Ok(BxValue::new_null())
            }
            "close" => {
                let code = args
                    .first()
                    .map(|value| vm.to_string(*value).parse::<u16>().unwrap_or(1000))
                    .unwrap_or(1000);
                let reason = args
                    .get(1)
                    .map(|value| vm.to_string(*value))
                    .unwrap_or_default();
                if let Some(sender) = self.outbound.borrow().get(&self.connection_id) {
                    sender
                        .send_bounded(WebSocketOutbound::Close { code, reason })
                        .map_err(|_| "WebSocket connection is closed".to_string())?;
                } else {
                    return Err("WebSocket connection is closed".to_string());
                }
                Ok(BxValue::new_null())
            }
            "getid" => Ok(BxValue::new_ptr(vm.string_new(self.connection_id.clone()))),
            "getpath" => Ok(BxValue::new_ptr(vm.string_new(self.request.path.clone()))),
            "geturl" => Ok(BxValue::new_ptr(
                vm.string_new(self.request.full_url.clone()),
            )),
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
    if *runtime.shutdown.borrow() {
        return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    let request = request_data_from_parts(method, uri.path(), uri.query(), &headers, Vec::new());
    let permit = match runtime.connections.clone().try_acquire_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "WebSocket connection limit reached",
            )
                .into_response();
        }
    };
    if let Some(authorize) = runtime.authorize.clone() {
        let candidate = request.clone();
        match tokio::task::spawn_blocking(move || authorize(candidate)).await {
            Ok(Ok(response)) if response.status == 200 => {}
            Ok(Ok(response)) => return response_to_http(response),
            _ => {
                return (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    "WebSocket authorization unavailable",
                )
                    .into_response();
            }
        }
    }
    let connection_id = Uuid::new_v4().to_string();
    ws.max_message_size(64 * 1024)
        .max_frame_size(64 * 1024)
        .on_upgrade(move |socket| async move {
            let _permit = permit;
            websocket_connection_loop(socket, runtime, connection_id, request).await;
        })
        .into_response()
}

pub async fn websocket_connection_loop(
    mut socket: WebSocket,
    runtime: Arc<WebSocketRuntimeHandle>,
    connection_id: String,
    request: RequestData,
) {
    let (outbound_tx, mut outbound_rx): (Sender<WebSocketOutbound>, Receiver<WebSocketOutbound>) =
        channel(16);
    let mut shutdown = runtime.shutdown.subscribe();
    if *shutdown.borrow() {
        let _ = socket.close().await;
        return;
    }

    if runtime
        .dispatch(WebSocketRuntimeCommand::Connect {
            connection_id: connection_id.clone(),
            request,
            outbound: outbound_tx,
        })
        .await
        .is_err()
    {
        let _ = socket.close().await;
        return;
    }

    loop {
        tokio::select! {
            _ = shutdown.changed() => { let _ = socket.close().await; break; }
            outbound = outbound_rx.recv() => {
                let Some(outbound) = outbound else {
                    break;
                };
                let send_result = tokio::time::timeout(std::time::Duration::from_secs(10), async {
                    match outbound {
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
                    }
                }).await;
                if !matches!(send_result, Ok(Ok(()))) { break; }
            }
            inbound = socket.recv() => {
                let Some(inbound) = inbound else {
                    break;
                };
                match inbound {
                    Ok(WebSocketMessage::Text(text)) => {
                        if runtime.dispatch(WebSocketRuntimeCommand::Message {
                            connection_id: connection_id.clone(),
                            message: IncomingWebSocketMessage::Text(text),
                        }).await.is_err() {
                            break;
                        }
                    }
                    Ok(WebSocketMessage::Binary(bytes)) => {
                        if runtime.dispatch(WebSocketRuntimeCommand::Message {
                            connection_id: connection_id.clone(),
                            message: IncomingWebSocketMessage::Binary(bytes),
                        }).await.is_err() {
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
        .dispatch(WebSocketRuntimeCommand::Close { connection_id })
        .await;
}

use crate::app_server::{BuildState, install_web_namespace};
use std::path::PathBuf;
use std::sync::Mutex;

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
    let state = SocketState::new(&mut vm, listener);
    while let Ok(command) = commands.recv() {
        state.command(&mut vm, command);
    }
    Ok(())
}

pub(crate) struct SocketState {
    listener: BxValue,
    registry: usize,
    outbound: Rc<RefCell<HashMap<String, Sender<WebSocketOutbound>>>>,
}
impl SocketState {
    pub(crate) fn new(vm: &mut VM, listener: BxValue) -> Self {
        vm.insert_global("__websocketlistener".to_string(), listener);
        let channel_registry_id = vm.struct_new();
        vm.insert_global(
            "__websocketconnections".to_string(),
            BxValue::new_ptr(channel_registry_id),
        );

        let outbound_senders: Rc<RefCell<HashMap<String, Sender<WebSocketOutbound>>>> =
            Rc::new(RefCell::new(HashMap::new()));

        Self {
            listener,
            registry: channel_registry_id,
            outbound: outbound_senders,
        }
    }
    pub(crate) fn command(&self, mut vm: &mut VM, command: WebSocketRuntimeCommand) {
        let listener = self.listener;
        let channel_registry_id = self.registry;
        let outbound_senders = &self.outbound;
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
                    send_websocket_close(
                        outbound_senders.clone(),
                        &connection_id,
                        1011,
                        "Internal error",
                    );
                }
            }
            WebSocketRuntimeCommand::Message {
                connection_id,
                message,
            } => {
                let channel = vm.struct_get(channel_registry_id, &connection_id);
                if channel.is_null() {
                    return;
                }
                let message_value = match message {
                    IncomingWebSocketMessage::Text(text) => BxValue::new_ptr(vm.string_new(text)),
                    IncomingWebSocketMessage::Binary(bytes) => {
                        BxValue::new_ptr(vm.bytes_new(bytes))
                    }
                };
                if let Err(err) =
                    vm.call_method_value(listener, "onmessage", vec![message_value, channel])
                {
                    eprintln!("WebSocket onMessage error: {}", err);
                    send_websocket_close(
                        outbound_senders.clone(),
                        &connection_id,
                        1011,
                        "Internal error",
                    );
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
}

fn build_websocket_channel(
    vm: &mut VM,
    connection_id: &str,
    request: RequestData,
    outbound: Rc<RefCell<HashMap<String, Sender<WebSocketOutbound>>>>,
) -> BxValue {
    let id = vm.native_object_new(Rc::new(RefCell::new(WebSocketChannelObject {
        connection_id: connection_id.to_string(),
        request,
        outbound,
    })));
    BxValue::new_ptr(id)
}

fn send_websocket_close(
    outbound: Rc<RefCell<HashMap<String, Sender<WebSocketOutbound>>>>,
    connection_id: &str,
    code: u16,
    reason: &str,
) {
    let failed = outbound.borrow().get(connection_id).is_some_and(|sender| {
        sender
            .send_bounded(WebSocketOutbound::Close {
                code,
                reason: reason.to_string(),
            })
            .is_err()
    });
    if failed {
        outbound.borrow_mut().remove(connection_id);
    }
}
