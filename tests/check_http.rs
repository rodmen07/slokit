//! End-to-end `check` tests against an in-process mock HTTP server, so the
//! Prometheus client path is exercised for real without external services.

#![cfg(feature = "check")]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;

use slokit::check::{check_slo, PrometheusClient, StatusLevel};
use slokit::spec::Spec;
use slokit::Window;

/// Serve `conns` connections that each return `body` as a JSON response, then
/// stop. `Connection: close` forces a fresh connection per request.
fn spawn_mock(body: &'static str, conns: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        for _ in 0..conns {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// Serve `conns` connections with a configurable non-200 status and JSON/text
/// body, then stop.
fn spawn_mock_status(body: &'static str, status: &str, conns: usize) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let status = status.to_string();
    thread::spawn(move || {
        for _ in 0..conns {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

/// Serve one connection, return the raw request bytes to the caller, and
/// respond with the provided JSON body.
fn spawn_mock_capture_request(body: &'static str) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 2048];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        let _ = tx.send(request);

        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
    });

    (port, rx)
}

const VECTOR_0_0005: &str = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"0.0005"]}]}}"#;
const VECTOR_NAN: &str = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"NaN"]}]}}"#;
const VECTOR_POS_INF: &str = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"+Inf"]}]}}"#;

#[test]
fn query_scalar_reads_value_over_http() {
    let body = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"0.0042"]}]}}"#;
    let port = spawn_mock(body, 1);
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}")).unwrap();
    assert_eq!(client.query_scalar("up").unwrap(), Some(0.0042));
}

#[test]
fn check_slo_computes_budget_and_burn() {
    // Two queries per SLO (period + current window), both return 0.0005.
    let port = spawn_mock(VECTOR_0_0005, 2);
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}")).unwrap();

    let spec = Spec::from_yaml(
        r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#,
    )
    .unwrap();

    let status = check_slo(
        &client,
        &spec.service,
        &spec.slos[0],
        Window::days(30),
        Window::hours(1),
    )
    .unwrap();

    assert_eq!(status.service, "s");
    // Budget ratio is 0.001; an observed 0.0005 ratio consumes half the budget.
    assert!((status.budget_consumed_ratio.unwrap() - 0.5).abs() < 1e-9);
    assert!((status.budget_remaining_ratio.unwrap() - 0.5).abs() < 1e-9);
    assert!((status.current_burn_rate.unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(status.level, StatusLevel::Ok);
}

#[test]
fn query_scalar_sends_bearer_auth_header() {
    let body = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"0.0042"]}]}}"#;
    let (port, rx) = spawn_mock_capture_request(body);
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}"))
        .unwrap()
        .with_bearer_token("top-secret-token");

    let value = client.query_scalar("up").unwrap();
    assert_eq!(value, Some(0.0042));

    let request = rx.recv().unwrap().to_ascii_lowercase();
    assert!(request.contains("authorization: bearer top-secret-token"));
}

#[test]
fn query_scalar_rejects_nan_from_http_response() {
    let port = spawn_mock(VECTOR_NAN, 1);
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}")).unwrap();

    let err = client.query_scalar("up").unwrap_err();
    assert!(err.to_string().contains("non-finite sample value"));
}

#[test]
fn check_slo_rejects_infinite_sample_from_http_response() {
    let port = spawn_mock(VECTOR_POS_INF, 2);
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}")).unwrap();

    let spec = Spec::from_yaml(
        r#"
service: s
slos:
  - name: a
    objective: 99.9
    sli:
      raw:
        error_ratio_query: my_ratio[{{.window}}]
"#,
    )
    .unwrap();

    let err = check_slo(
        &client,
        &spec.service,
        &spec.slos[0],
        Window::days(30),
        Window::hours(1),
    )
    .unwrap_err();
    assert!(err.to_string().contains("non-finite sample value"));
}

#[test]
fn query_scalar_http_error_includes_status_and_response_body() {
    let port = spawn_mock_status(
        "{\"error\":\"prometheus upstream unavailable\"}",
        "503 Service Unavailable",
        1,
    );
    let client = PrometheusClient::new(format!("http://127.0.0.1:{port}")).unwrap();

    let err = client.query_scalar("up").unwrap_err().to_string();
    assert!(err.contains("HTTP 503 Service Unavailable"));
    assert!(err.contains("prometheus upstream unavailable"));
}
