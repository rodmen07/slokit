//! End-to-end `check` tests against an in-process mock HTTP server, so the
//! Prometheus client path is exercised for real without external services.

#![cfg(feature = "check")]

use std::io::{Read, Write};
use std::net::TcpListener;
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

const VECTOR_0_0005: &str = r#"{"status":"success","data":{"resultType":"vector","result":[{"metric":{},"value":[1719000000,"0.0005"]}]}}"#;

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

    let status = check_slo(&client, &spec.slos[0], Window::days(30), Window::hours(1)).unwrap();

    // Budget ratio is 0.001; an observed 0.0005 ratio consumes half the budget.
    assert!((status.budget_consumed_ratio.unwrap() - 0.5).abs() < 1e-9);
    assert!((status.budget_remaining_ratio.unwrap() - 0.5).abs() < 1e-9);
    assert!((status.current_burn_rate.unwrap() - 0.5).abs() < 1e-9);
    assert_eq!(status.level, StatusLevel::Ok);
}
