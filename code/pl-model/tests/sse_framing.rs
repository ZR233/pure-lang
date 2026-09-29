//! Public model completion over deliberately fragmented HTTP/SSE framing.
use pl_model::completion::{CompletionRequest, Message, MessageContent, MessageRole};
use pl_model::model::{ModelInfo, ModelTransportProfile};
use pl_model::provider::ProviderEndpoint;
use pl_model::runtime::{ModelInvocationContext, ModelRuntime, ModelSession};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[tokio::test]
async fn fragmented_sse_preserves_unicode_multiline_data_and_all_line_endings() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = async {
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
        socket
            .read_exact(&mut vec![0; content_length])
            .await
            .unwrap();
        let mut socket = socket.into_inner();
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
