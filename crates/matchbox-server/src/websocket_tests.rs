#[cfg(test)]
mod tests {
    use crate::websocket::*;
    use matchbox_compiler::compiler::Compiler;
    use matchbox_compiler::parser;
    use std::sync::mpsc;
    use std::thread;
    use tokio::sync::mpsc::unbounded_channel;
    use crate::RequestData;
    use std::collections::HashMap;

    #[test]
    fn test_websocket_runtime_echo() {
        let source = r#"
            class EchoListener {
                function onConnect(channel) {
                    channel.sendMessage("welcome");
                }
                function onMessage(msg, channel) {
                    channel.sendMessage("echo:" + msg);
                }
                function onClose(channel) {}
            }
        "#;
        let ast = parser::parse(source).unwrap();
        let mut compiler = Compiler::new("test.bxs");
        let chunk = compiler.compile(&ast, source).unwrap();

        let config = WebSocketConfig {
            uri: "/ws".to_string(),
            listener_class: "EchoListener".to_string(),
            listener_state: serde_json::json!({}),
        };

        let (cmd_tx, cmd_rx) = mpsc::channel();
        
        let handle = thread::spawn(move || {
            websocket_runtime_main(chunk, config, cmd_rx, None).unwrap();
        });

        let (out_tx, mut out_rx) = unbounded_channel();
        let connection_id = "test-conn".to_string();
        
        // Connect
        cmd_tx.send(WebSocketRuntimeCommand::Connect {
            connection_id: connection_id.clone(),
            request: RequestData {
                method: "GET".to_string(),
                path: "/ws".to_string(),
                matched_route: None,
                route_params: HashMap::new(),
                raw_query: None,
                query: HashMap::new(),
                cookies: HashMap::new(),
                headers: HashMap::new(),
                body: Vec::new(),
                full_url: "http://localhost/ws".to_string(),
            },
            outbound: out_tx,
        }).unwrap();

        // Should get welcome
        let msg = out_rx.blocking_recv().unwrap();
        if let WebSocketOutbound::Text(t) = msg {
            assert_eq!(t, "welcome");
        } else {
            panic!("Expected text message");
        }

        // Send message
        cmd_tx.send(WebSocketRuntimeCommand::Message {
            connection_id: connection_id.clone(),
            message: IncomingWebSocketMessage::Text("hello".to_string()),
        }).unwrap();

        // Should get echo
        let msg = out_rx.blocking_recv().unwrap();
        if let WebSocketOutbound::Text(t) = msg {
            assert_eq!(t, "echo:hello");
        } else {
            panic!("Expected text message");
        }

        // Close
        drop(cmd_tx);
        handle.join().unwrap();
    }

    #[test]
    fn test_websocket_broadcast() {
        let source = r#"
            class BroadcastListener {
                function onConnect(channel) {}
                function onMessage(msg, channel) {
                    channel.broadcastMessage("all:" + msg);
                }
                function onClose(channel) {}
            }
        "#;
        let ast = parser::parse(source).unwrap();
        let mut compiler = Compiler::new("test.bxs");
        let chunk = compiler.compile(&ast, source).unwrap();

        let config = WebSocketConfig {
            uri: "/ws".to_string(),
            listener_class: "BroadcastListener".to_string(),
            listener_state: serde_json::json!({}),
        };

        let (cmd_tx, cmd_rx) = mpsc::channel();
        let _handle = thread::spawn(move || {
            websocket_runtime_main(chunk, config, cmd_rx, None).unwrap();
        });

        let (out1_tx, mut out1_rx) = unbounded_channel();
        let (out2_tx, mut out2_rx) = unbounded_channel();
        
        let req = RequestData {
            method: "GET".to_string(),
            path: "/ws".to_string(),
            matched_route: None,
            route_params: HashMap::new(),
            raw_query: None,
            query: HashMap::new(),
            cookies: HashMap::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            full_url: "http://localhost/ws".to_string(),
        };

        cmd_tx.send(WebSocketRuntimeCommand::Connect {
            connection_id: "conn1".to_string(),
            request: req.clone(),
            outbound: out1_tx,
        }).unwrap();

        cmd_tx.send(WebSocketRuntimeCommand::Connect {
            connection_id: "conn2".to_string(),
            request: req,
            outbound: out2_tx,
        }).unwrap();

        // Send message from conn1
        cmd_tx.send(WebSocketRuntimeCommand::Message {
            connection_id: "conn1".to_string(),
            message: IncomingWebSocketMessage::Text("hi".to_string()),
        }).unwrap();

        // Both should get it
        let msg1 = out1_rx.blocking_recv().unwrap();
        let msg2 = out2_rx.blocking_recv().unwrap();

        if let WebSocketOutbound::Text(t) = msg1 {
            assert_eq!(t, "all:hi");
        }
        if let WebSocketOutbound::Text(t) = msg2 {
            assert_eq!(t, "all:hi");
        }
    }

    #[tokio::test]
    async fn test_regular_server_websocket_routing() {
        use axum::routing::get;
        use axum::Router;
        use std::sync::Arc;
        use tokio_tungstenite::tungstenite::Message;
        use futures_util::SinkExt;

        let (cmd_tx, _cmd_rx) = mpsc::channel();
        let runtime = Arc::new(WebSocketRuntimeHandle {
            uri: "/ws".to_string(),
            commands: cmd_tx,
        });

        let mut router = Router::new();
        router = router.route(&runtime.uri, get(websocket_handler).with_state(runtime.clone()));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        
        let server_handle = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });

        let url = format!("ws://{}/ws", addr);
        let conn = tokio_tungstenite::connect_async(&url).await;
        
        if let Ok((mut stream, _)) = conn {
            let _ = stream.send(Message::Text("test".to_string())).await;
            let _ = stream.close(None).await;
        }

        server_handle.abort();
    }
}
