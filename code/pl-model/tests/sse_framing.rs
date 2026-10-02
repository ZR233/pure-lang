//! Public model completion over deliberately fragmented HTTP/SSE framing.
use pl_model::completion::{CompletionRequest, Message, MessageContent, MessageRole};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::ProviderEndpoint;
use pl_model::runtime::{ModelInvocationContext, ModelRuntime, ModelSession};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

async fn read_request(listener: &TcpListener) -> (TcpStream, Vec<u8>) {
    let (socket, _) = listener.accept().await.unwrap();
    let mut socket = BufReader::new(socket);
    let mut content_length = 0;
    loop {
        let mut header = String::new();
        assert_ne!(socket.read_line(&mut header).await.unwrap(), 0);
        if header == "\r\n" {
            break;
        }
        if let Some((name, value)) = header.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            content_length = value.trim().parse::<usize>().unwrap();
            assert!(content_length < 16_384);
        }
    }
    let mut request = vec![0; content_length];
    socket.read_exact(&mut request).await.unwrap();
    (socket.into_inner(), request)
}

#[tokio::test]
async fn http_body_failures_retry_but_invalid_sse_json_does_not() {
    // Raw body failures, model syntax errors, billing, and HTTP status have
    // different replay semantics even when the connection ends the same way.
    for (damaged_chunk, status, billed, should_retry) in [
        (Some("10\r\ndata:"), 200, false, true),
        (Some("invalid\r\n"), 200, false, true),
        (None, 200, false, false),
        (Some("invalid\r\n"), 200, true, false),
        (Some("invalid\r\n"), 401, false, false),
        (Some("invalid\r\n"), 503, false, true),
    ] {
        let interrupted = damaged_chunk.is_some();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = async move {
            let mut requests = Vec::new();
            loop {
                let (mut socket, request) = read_request(&listener).await;
                requests.push(request);
                let status = if requests.len() == 1 { status } else { 200 };
                socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\nRetry-After-Ms: 1\r\nx-request-id: fixture-body\r\n\r\n").as_bytes()).await.unwrap();
                let body = if requests.len() == 1 {
                    if billed {
                        "data: {\"id\":\"first\",\"choices\":[],\"usage\":{\"prompt_tokens\":17,\"completion_tokens\":7,\"total_tokens\":24}}\n\n"
                    } else if interrupted {
                        "data: {\"id\":\"first\",\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n"
                    } else {
                        "data: {invalid json}\n\n"
                    }
                } else {
                    concat!(
                        "data: {\"id\":\"second\",\"choices\":[{\"delta\":{\"content\":\"recovered\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"second\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: [DONE]\n\n",
                    )
                };
                socket
                    .write_all(format!("{:x}\r\n{body}\r\n", body.len()).as_bytes())
                    .await
                    .unwrap();
                if interrupted && requests.len() == 1 {
                    // Both truncation and corrupt HTTP chunk framing happen
                    // before model JSON decoding and must use the retry budget.
                    socket
                        .write_all(damaged_chunk.unwrap().as_bytes())
                        .await
                        .unwrap();
                    socket.shutdown().await.unwrap();
                    if should_retry {
                        continue;
                    }
                    return requests;
                }
                socket.write_all(b"0\r\n\r\n").await.unwrap();
                return requests;
            }
        };
        let client = async {
            let mut model = ModelInfo::compatible("fixture");
            model
                .binding
                .set_transport(ModelTransportProfile::chat_completions_http());
            let result =
                ModelRuntime::new(ProviderEndpoint::compatible("fixture", endpoint), model)
                    .unwrap()
                    .complete(
                        CompletionRequest::builder()
                            .messages(vec![Message {
                                presentation: Default::default(),
                                role: MessageRole::User,
                                content: MessageContent::text("hello"),
                                reasoning_content: None,
                                tool_calls: None,
                                tool_result: None,
                                metadata: Default::default(),
                            }])
                            .build(),
                        ModelInvocationContext::new(ModelSession::default()),
                    )
                    .await;
            if should_retry {
                let response = result.unwrap();
                assert_eq!(response.content.as_deref(), Some("recovered"));
                assert_eq!(response.orchestration.transport_attempts, 2);
            } else {
                let failure = result.unwrap_err();
                if billed {
                    assert!(failure.source.is_transient_model_transport());
                    assert_eq!(failure.accounting.usage.total_tokens, Some(24));
                    let provider = failure.source.provider_failure_ref().unwrap();
                    assert_eq!(provider.http_status, Some(200));
                    assert_eq!(provider.context.request_id.as_deref(), Some("fixture-body"));
                    // Hyper may report malformed chunk framing as InvalidInput
                    // or UnexpectedEof; both must retain the underlying IO cause.
                    assert!(
                        provider.message.contains("io=InvalidInput")
                            || provider.message.contains("io=UnexpectedEof"),
                        "{}",
                        provider.message
                    );
                    assert!(
                        provider.message.contains("version=HTTP/1.1"),
                        "{}",
                        provider.message
                    );
                    assert!(provider.message.contains("received_bytes="));
                    assert!(!provider.message.contains("prompt_tokens"));
                } else if status == 401 {
                    let provider = failure.source.provider_failure_ref().unwrap();
                    assert_eq!(provider.http_status, Some(401));
                    assert!(!provider.retry.is_retryable());
                } else {
                    assert!(
                        matches!(*failure.source, pl_model::PureError::Protocol(_)),
                        "{failure:?}"
                    );
                }
            }
        };
        let (requests, ()) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(server, client)
        })
        .await
        .expect("HTTP completion terminates");
        if should_retry {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], requests[1]);
        } else {
            assert_eq!(requests.len(), 1);
        }
    }
}

#[tokio::test]
async fn native_compaction_retries_frozen_history_only_before_receiving_checkpoint() {
    for checkpoint_received in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let server = async move {
            let mut requests = Vec::new();
            for attempt in 0..if checkpoint_received { 1 } else { 2 } {
                let (mut socket, request) = read_request(&listener).await;
                requests.push(request);
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                let body = if attempt == 0 && checkpoint_received {
                    concat!(
                        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"interrupted\"}}\n\n",
                        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"checkpoint\"}}\n\n",
                    )
                } else if attempt == 0 {
                    "data: {\"type\":\"response.created\",\"response\":{\"id\":\"interrupted\"}}\n\n"
                } else {
                    concat!(
                        "data: {\"type\":\"response.created\",\"response\":{\"id\":\"recovered\"}}\n\n",
                        "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"compaction\",\"encrypted_content\":\"checkpoint\"}}\n\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"id\":\"recovered\",\"usage\":{\"input_tokens\":20,\"output_tokens\":3}}}\n\n",
                    )
                };
                let ending = if attempt == 0 {
                    "invalid\r\n"
                } else {
                    "0\r\n\r\n"
                };
                socket
                    .write_all(format!("{:x}\r\n{body}\r\n{ending}", body.len()).as_bytes())
                    .await
                    .unwrap();
                socket.shutdown().await.unwrap();
            }
            requests
        };
        let client = async {
            let mut model = ModelInfo::compatible("fixture");
            model
                .binding
                .set_transport(ModelTransportProfile::responses_http());
            let mut endpoint = ProviderEndpoint::compatible("fixture", endpoint);
            endpoint.service_capabilities.remote_compaction = true;
            let runtime = ModelRuntime::new(endpoint, model).unwrap();
            let request = CompletionRequest::builder()
                .messages(vec![Message {
                    presentation: Default::default(),
                    role: MessageRole::User,
                    content: MessageContent::text("retained task history"),
                    reasoning_content: None,
                    tool_calls: None,
                    tool_result: None,
                    metadata: Default::default(),
                }])
                .build();
            let result = runtime
                .compaction()
                .unwrap()
                .checkpoint(request, ModelInvocationContext::default())
                .await;
            if checkpoint_received {
                let failure = result.unwrap_err();
                let provider = failure.source.provider_failure_ref().unwrap();
                assert_eq!(provider.http_status, Some(200));
                assert!(
                    provider.message.contains("io=InvalidInput")
                        || provider.message.contains("io=UnexpectedEof"),
                    "{}",
                    provider.message
                );
                assert!(failure.accounting.usage.input_tokens.is_none());
                return;
            }
            let compacted = result.unwrap();
            assert!(
                matches!(compacted.item, pl_model::completion::ModelContextItem::Compaction { encrypted_content } if encrypted_content == "checkpoint")
            );
            assert_eq!(compacted.accounting.usage.input_tokens, Some(20));
            assert_eq!(compacted.accounting.usage.output_tokens, Some(3));
        };
        let (requests, ()) = tokio::time::timeout(Duration::from_secs(10), async {
            tokio::join!(server, client)
        })
        .await
        .expect("compaction recovers");
        if !checkpoint_received {
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], requests[1]);
        } else {
            assert_eq!(requests.len(), 1);
        }
        let body: serde_json::Value = serde_json::from_slice(&requests[0]).unwrap();
        assert_eq!(
            body["input"].as_array().unwrap().last().unwrap()["type"],
            "compaction_trigger"
        );
    }
}

#[tokio::test]
async fn fragmented_sse_preserves_unicode_multiline_data_and_all_line_endings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = async {
        let (mut socket, _) = read_request(&listener).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        // One-byte HTTP chunks split both CRLF and multibyte UTF-8 characters.
        // Multiline data must remain one JSON event; comments are not events.
        let body = concat!(
            ": comment\r\n",
            "data: {\"id\":\"chat-1\",\"model\":\"fixture\",\r",
            "data: \"choices\":[{\"delta\":{\"content\":\"你好\"},\"finish_reason\":null}]}\r\r",
            "data: {\"id\":\"chat-1\",\"model\":\"fixture\",\"choices\":[{\"delta\":{\"content\":\" 🌍\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"chat-1\",\"model\":\"fixture\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\r\n\r\n",
            "data: [DONE]\r\n\r\n",
        );
        for byte in body.bytes() {
            socket
                .write_all(&[b'1', b'\r', b'\n', byte, b'\r', b'\n'])
                .await
                .unwrap();
        }
        socket.write_all(b"0\r\n\r\n").await.unwrap();
    };
    let client = async {
        let mut model = ModelInfo::compatible("fixture");
        model
            .binding
            .set_transport(ModelTransportProfile::chat_completions_http());
        let runtime =
            ModelRuntime::new(ProviderEndpoint::compatible("fixture", endpoint), model).unwrap();
        runtime
            .complete(
                CompletionRequest::builder()
                    .messages(vec![Message {
                        presentation: Default::default(),
                        role: MessageRole::User,
                        content: MessageContent::text("hello"),
                        reasoning_content: None,
                        tool_calls: None,
                        tool_result: None,
                        metadata: Default::default(),
                    }])
                    .build(),
                ModelInvocationContext::new(ModelSession::default()),
            )
            .await
            .unwrap()
    };
    // Both futures belong to this scope: a timeout drops the listener, socket,
    // decoder and client together, without leaving a spawned server behind.
    let (_, response) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(server, client)
    })
    .await
    .expect("fragmented SSE completes");
    assert_eq!(response.content.as_deref(), Some("你好 🌍"));
}
