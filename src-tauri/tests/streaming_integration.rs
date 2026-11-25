use archify_lib::providers::cloud_http::CloudHttpPlannerReviewer;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

#[tokio::test]
async fn http_stream_emits_incremental_deltas_before_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        // Read request (ignore).
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;
        // Send SSE response with delays.
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket
            .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        socket
            .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        socket.write_all(b"data: [DONE]\n\n").await.unwrap();
    });

    let endpoint = format!("http://{}", addr);
    let client = CloudHttpPlannerReviewer::new(endpoint, "test-model".to_string(), None);
    // local endpoint needs no key
    let messages = vec![("user".to_string(), "hi".to_string())];
    let deltas: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let deltas_clone = deltas.clone();
    let (_tx, mut cancel_rx) = watch::channel(false);

    let result = client
        .chat_stream(messages, 30, &mut cancel_rx, move |d| {
            deltas_clone.lock().unwrap().push(d);
        })
        .await;

    server.await.unwrap();
    let deltas = deltas.lock().unwrap().clone();
    assert!(result.is_ok(), "result={:?}", result);
    assert_eq!(result.unwrap(), "Hello world");
    assert_eq!(deltas, vec!["Hello".to_string(), " world".to_string()]);
}

#[tokio::test]
async fn http_stream_handles_premature_closure() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket
            .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n")
            .await
            .unwrap();
        // close without [DONE]
    });
    let endpoint = format!("http://{}", addr);
    let client = CloudHttpPlannerReviewer::new(endpoint, "test-model".to_string(), None);
    let messages = vec![("user".to_string(), "hi".to_string())];
    let (_tx, mut cancel_rx) = watch::channel(false);
    let result = client
        .chat_stream(messages, 30, &mut cancel_rx, |_| {})
        .await;
    server.await.unwrap();
    // We chose to return Ok(partial) on premature closure.
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "partial");
}

#[tokio::test]
async fn http_stream_cancellation_stops() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 4096];
        let _ = socket.read(&mut buf).await;
        let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\n";
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket
            .write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"start\"}}]}\n\n")
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    });
    let endpoint = format!("http://{}", addr);
    let client = CloudHttpPlannerReviewer::new(endpoint, "test-model".to_string(), None);
    let messages = vec![("user".to_string(), "hi".to_string())];
    let (tx, mut cancel_rx) = watch::channel(false);
    let handle = tokio::spawn(async move {
        client
            .chat_stream(messages, 30, &mut cancel_rx, |_| {})
            .await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    tx.send(true).unwrap();
    let result = handle.await.unwrap();
    server.abort();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_lowercase().contains("cancel"));
}
