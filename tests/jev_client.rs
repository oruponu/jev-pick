use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use jev_pick::decision::{DecisionRequest, OptionId, RawDecisionInput};
use jev_pick::jev::{JevClient, JevClientTimings, JevError};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const SUCCESS: &str = include_str!("fixtures/choice_success.json");
const TIE: &str = include_str!("fixtures/choice_tie.json");

fn request_with_context() -> DecisionRequest {
    DecisionRequest::try_from(RawDecisionInput {
        question: "今日の\"晩ごはん\"は何にする？\n候補から選んで".into(),
        a: "カレー\\ライス".into(),
        b: "うどん".into(),
        c: Some("寿司".into()),
        d: None,
        context: Some("寒いので、温かい麺類が食べたい".into()),
    })
    .unwrap()
}

fn two_option_request(question: &str) -> DecisionRequest {
    DecisionRequest::try_from(RawDecisionInput {
        question: question.into(),
        a: "Tea".into(),
        b: "Coffee".into(),
        c: None,
        d: None,
        context: None,
    })
    .unwrap()
}

fn fast_timings() -> JevClientTimings {
    JevClientTimings {
        connect_timeout: Duration::from_millis(100),
        attempt_timeout: Duration::from_millis(250),
        total_timeout: Duration::from_millis(750),
        retry_delays: [Duration::from_millis(5), Duration::from_millis(10)],
    }
}

fn client(server: &MockServer) -> JevClient {
    JevClient::with_endpoint_and_timings(
        "secret-key",
        "jev-latest",
        &format!("{}/v1/systemone", server.uri()),
        fast_timings(),
    )
    .unwrap()
}

fn success_response() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(SUCCESS, "application/json")
}

#[tokio::test]
async fn posts_exact_contract_and_converts_success_response() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/systemone"))
        .respond_with(success_response())
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap();

    assert_eq!(result.selected_id, OptionId::B);
    assert_eq!(result.probabilities.len(), 3);
    assert_eq!(result.probabilities[&OptionId::A], 0.10);
    assert_eq!(result.probabilities[&OptionId::B], 0.85);
    assert_eq!(result.probabilities[&OptionId::C], 0.05);
    assert_eq!(result.confidence, 0.80);
    assert_eq!(result.model, "jev-1.13.0");
    assert_eq!(result.usage.input_tokens, 160);
    assert_eq!(result.usage.output_tokens, 32);

    let requests = server.received_requests().await.unwrap();
    let sent = &requests[0];
    assert_eq!(sent.url.path(), "/v1/systemone");
    assert_eq!(
        sent.headers.get("authorization").unwrap().to_str().unwrap(),
        "Bearer secret-key"
    );
    assert_eq!(
        sent.headers.get("content-type").unwrap().to_str().unwrap(),
        "application/json"
    );
    let body: Value = serde_json::from_slice(&sent.body).unwrap();
    assert_eq!(
        body,
        json!({
            "model": "jev-latest",
            "state": {"context": "寒いので、温かい麺類が食べたい"},
            "questions": {
                "decision": {
                    "type": "choice",
                    "instructions": "今日の\"晩ごはん\"は何にする？\n候補から選んで",
                    "criteria": {
                        "A": "カレー\\ライス",
                        "B": "うどん",
                        "C": "寿司"
                    }
                }
            }
        })
    );
}

#[tokio::test]
async fn sends_language_specific_fallback_context_without_unused_options() {
    for (question, fallback) in [
        ("Tea or coffee?", "No additional context was provided."),
        ("お茶かコーヒー？", "追加の条件は指定されていません。"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/systemone"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(TIE, "application/json"))
            .mount(&server)
            .await;

        client(&server)
            .decide(&two_option_request(question))
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&requests[0].body).unwrap();
        assert_eq!(body["state"]["context"], fallback);
        assert_eq!(
            body["questions"]["decision"]["criteria"],
            json!({"A": "Tea", "B": "Coffee"})
        );
    }
}

#[tokio::test]
async fn retains_the_api_choice_when_maximum_probabilities_are_tied() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(TIE, "application/json"))
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&two_option_request("Tea or coffee?"))
        .await
        .unwrap();
    assert_eq!(result.selected_id, OptionId::B);
}

#[tokio::test]
async fn accepts_unknown_response_fields() {
    let server = MockServer::start().await;
    let mut body: Value = serde_json::from_str(SUCCESS).unwrap();
    body["future_top_level"] = json!({"enabled": true});
    body["answers"]["decision"]["future_answer_field"] = json!(42);
    body["usage"]["future_usage_field"] = json!(7);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap();
    assert_eq!(result.selected_id, OptionId::B);
}

#[tokio::test]
async fn rejects_malformed_or_contract_violating_responses() {
    let valid: Value = serde_json::from_str(SUCCESS).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("invalid JSON", b"not-json".to_vec()),
        (
            "missing model",
            mutate(&valid, |v| {
                v.as_object_mut().unwrap().remove("model");
            }),
        ),
        ("empty model", mutate(&valid, |v| v["model"] = json!(""))),
        (
            "missing answer",
            mutate(&valid, |v| {
                v["answers"].as_object_mut().unwrap().remove("decision");
            }),
        ),
        (
            "wrong answer type",
            mutate(&valid, |v| v["answers"]["decision"]["type"] = json!("text")),
        ),
        (
            "unknown choice",
            mutate(&valid, |v| v["answers"]["decision"]["choice"] = json!("D")),
        ),
        (
            "missing probability",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]
                    .as_object_mut()
                    .unwrap()
                    .remove("C");
            }),
        ),
        (
            "extra probability",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]["D"] = json!(0.0)
            }),
        ),
        (
            "negative probability",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]["A"] = json!(-0.1);
                v["answers"]["decision"]["probabilities"]["B"] = json!(1.05);
            }),
        ),
        (
            "probability above one",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]["A"] = json!(1.01);
                v["answers"]["decision"]["probabilities"]["B"] = json!(-0.06);
            }),
        ),
        (
            "invalid sum",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]["A"] = json!(0.11)
            }),
        ),
        (
            "choice is not maximum",
            mutate(&valid, |v| {
                v["answers"]["decision"]["probabilities"]["A"] = json!(0.6);
                v["answers"]["decision"]["probabilities"]["B"] = json!(0.35);
            }),
        ),
        (
            "negative confidence",
            mutate(&valid, |v| {
                v["answers"]["decision"]["confidence"] = json!(-0.1)
            }),
        ),
        (
            "confidence above one",
            mutate(&valid, |v| {
                v["answers"]["decision"]["confidence"] = json!(1.1)
            }),
        ),
        (
            "negative usage",
            mutate(&valid, |v| v["usage"]["input_tokens"] = json!(-1)),
        ),
        (
            "fractional usage",
            mutate(&valid, |v| v["usage"]["output_tokens"] = json!(1.5)),
        ),
        (
            "non-finite numeric syntax",
            SUCCESS.replace("0.80", "1e400").into_bytes(),
        ),
    ];

    for (name, body) in cases {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/json"))
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server)
            .decide(&request_with_context())
            .await
            .unwrap_err();
        assert_eq!(error, JevError::InvalidResponse, "case: {name}");
    }
}

fn mutate(value: &Value, change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut changed = value.clone();
    change(&mut changed);
    serde_json::to_vec(&changed).unwrap()
}

fn padded_success_body(size: usize) -> Vec<u8> {
    let mut body: Value = serde_json::from_str(SUCCESS).unwrap();
    body["ignored_padding"] = json!("");
    let empty = serde_json::to_vec(&body).unwrap();
    assert!(empty.len() <= size);
    body["ignored_padding"] = json!("x".repeat(size - empty.len()));
    let encoded = serde_json::to_vec(&body).unwrap();
    assert_eq!(encoded.len(), size);
    encoded
}

#[tokio::test]
async fn classifies_non_retryable_statuses_without_reading_or_retrying_bodies() {
    for (status, expected) in [
        (401, JevError::Authentication),
        (403, JevError::Authentication),
        (400, JevError::InvalidRequest),
        (422, JevError::InvalidRequest),
        (302, JevError::Transport),
        (500, JevError::Transport),
    ] {
        let server = MockServer::start().await;
        let response = if status == 302 {
            ResponseTemplate::new(status).insert_header("location", "/must-not-follow")
        } else {
            ResponseTemplate::new(status)
        };
        Mock::given(method("POST"))
            .respond_with(response.set_body_string("provider secret body"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/must-not-follow"))
            .respond_with(success_response())
            .expect(0)
            .mount(&server)
            .await;

        let error = client(&server)
            .decide(&request_with_context())
            .await
            .unwrap_err();
        assert_eq!(error, expected, "status: {status}");
    }
}

#[tokio::test]
async fn returns_non_retryable_status_before_a_stalled_error_body_arrives() {
    let stall = Duration::from_millis(500);
    let (endpoint, server_thread) = raw_stalled_error_body_server(401, stall);
    let client = JevClient::with_endpoint_and_timings(
        "key",
        "model",
        &endpoint,
        JevClientTimings {
            attempt_timeout: Duration::from_millis(750),
            total_timeout: Duration::from_secs(1),
            ..fast_timings()
        },
    )
    .unwrap();

    let started = Instant::now();
    let error = client.decide(&request_with_context()).await.unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(error, JevError::Authentication);
    assert!(elapsed < Duration::from_millis(250), "elapsed: {elapsed:?}");
    server_thread.join().unwrap();
}

#[derive(Clone)]
struct SequenceResponder {
    calls: Arc<AtomicUsize>,
    statuses: Vec<u16>,
}

impl Respond for SequenceResponder {
    fn respond(&self, _request: &Request) -> ResponseTemplate {
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        let status = self.statuses.get(index).copied().unwrap_or(500);
        if status == 200 {
            success_response()
        } else {
            ResponseTemplate::new(status).set_body_string("ignored provider body")
        }
    }
}

#[tokio::test]
async fn retries_429_then_success_with_byte_identical_payloads() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .respond_with(SequenceResponder {
            calls: Arc::clone(&calls),
            statuses: vec![429, 200],
        })
        .expect(2)
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap();
    assert_eq!(result.selected_id, OptionId::B);
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].body, requests[1].body);
}

#[tokio::test]
async fn retries_529_then_success() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .respond_with(SequenceResponder {
            calls: Arc::clone(&calls),
            statuses: vec![529, 200],
        })
        .expect(2)
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap();
    assert_eq!(result.selected_id, OptionId::B);
}

#[tokio::test]
async fn successful_retry_waits_for_the_configured_baseline() {
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .respond_with(SequenceResponder {
            calls: Arc::clone(&calls),
            statuses: vec![429, 200],
        })
        .expect(2)
        .mount(&server)
        .await;
    let client = JevClient::with_endpoint_and_timings(
        "key",
        "model",
        &format!("{}/v1/systemone", server.uri()),
        JevClientTimings {
            total_timeout: Duration::from_secs(1),
            retry_delays: [Duration::from_millis(100), Duration::from_millis(10)],
            ..fast_timings()
        },
    )
    .unwrap();

    let started = Instant::now();
    client.decide(&request_with_context()).await.unwrap();
    let elapsed = started.elapsed();

    assert!(elapsed >= Duration::from_millis(90), "elapsed: {elapsed:?}");
    assert!(elapsed < Duration::from_millis(500), "elapsed: {elapsed:?}");
}

#[tokio::test]
async fn retries_only_three_times_and_returns_the_last_retryable_status() {
    for (status, expected) in [(429, JevError::RateLimited), (529, JevError::Overloaded)] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(status))
            .expect(3)
            .mount(&server)
            .await;

        let error = client(&server)
            .decide(&request_with_context())
            .await
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn retry_after_that_reaches_deadline_stops_without_waiting_or_resending() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "1"))
        .expect(1)
        .mount(&server)
        .await;
    let timings = JevClientTimings {
        total_timeout: Duration::from_millis(60),
        ..fast_timings()
    };
    let client = JevClient::with_endpoint_and_timings(
        "key",
        "model",
        &format!("{}/v1/systemone", server.uri()),
        timings,
    )
    .unwrap();

    let started = Instant::now();
    let error = client.decide(&request_with_context()).await.unwrap_err();
    assert_eq!(error, JevError::RateLimited);
    assert!(started.elapsed() < Duration::from_millis(300));
}

#[tokio::test]
async fn per_attempt_timeout_includes_the_response_body() {
    let body = SUCCESS.as_bytes().to_vec();
    let (endpoint, server_thread) = raw_delayed_body_server(body, Duration::from_millis(150));
    let client = JevClient::with_endpoint_and_timings(
        "key",
        "model",
        &endpoint,
        JevClientTimings {
            attempt_timeout: Duration::from_millis(30),
            total_timeout: Duration::from_millis(200),
            ..fast_timings()
        },
    )
    .unwrap();

    let error = client.decide(&request_with_context()).await.unwrap_err();
    assert_eq!(error, JevError::Timeout);
    server_thread.join().unwrap();
}

#[tokio::test]
async fn total_timeout_stops_an_in_flight_attempt_before_its_longer_attempt_timeout() {
    let body = SUCCESS.as_bytes().to_vec();
    let (endpoint, server_thread) = raw_delayed_body_server(body, Duration::from_millis(300));
    let client = JevClient::with_endpoint_and_timings(
        "key",
        "model",
        &endpoint,
        JevClientTimings {
            attempt_timeout: Duration::from_millis(500),
            total_timeout: Duration::from_millis(60),
            ..fast_timings()
        },
    )
    .unwrap();

    let started = Instant::now();
    let error = client.decide(&request_with_context()).await.unwrap_err();
    let elapsed = started.elapsed();

    assert_eq!(error, JevError::Timeout);
    assert!(elapsed < Duration::from_millis(200), "elapsed: {elapsed:?}");
    server_thread.join().unwrap();
}

#[tokio::test]
async fn rejects_success_body_over_64_kib_from_content_length() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(padded_success_body(65_537), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let error = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap_err();
    assert_eq!(error, JevError::InvalidResponse);
}

#[tokio::test]
async fn accepts_valid_success_body_of_exactly_64_kib() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(padded_success_body(65_536), "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&request_with_context())
        .await
        .unwrap();
    assert_eq!(result.selected_id, OptionId::B);
}

#[tokio::test]
async fn rejects_actual_stream_over_64_kib_without_content_length() {
    let (endpoint, server_thread) = raw_chunked_server(padded_success_body(65_537));
    let client =
        JevClient::with_endpoint_and_timings("key", "model", &endpoint, fast_timings()).unwrap();

    let error = client.decide(&request_with_context()).await.unwrap_err();
    assert_eq!(error, JevError::InvalidResponse);
    server_thread.join().unwrap();
}

#[tokio::test]
async fn enforces_sum_and_selected_probability_tolerances_at_their_edges() {
    let sum_cases = [
        ("sum just inside", 0.999e-6, true),
        ("sum just outside", 1.001e-6, false),
    ];
    for (name, delta, accepted) in sum_cases {
        let a = 0.5;
        let b = 0.5 + delta;
        let actual_error = (a + b - 1.0_f64).abs();
        assert_eq!(actual_error <= 1e-6, accepted, "fixture: {name}");
        assert_response_acceptance(name, a, b, "B", accepted).await;
    }

    let choice_cases = [
        ("choice just inside", 0.999e-9, true),
        ("choice just outside", 1.001e-9, false),
    ];
    for (name, target_difference, accepted) in choice_cases {
        let a = 0.5 + target_difference / 2.0;
        let b = 1.0 - a;
        let actual_difference = a - b;
        assert_eq!(
            actual_difference <= 1e-9,
            accepted,
            "fixture: {name}, actual difference: {actual_difference:e}"
        );
        assert_response_acceptance(name, a, b, "B", accepted).await;
    }
}

async fn assert_response_acceptance(name: &str, a: f64, b: f64, choice: &str, accepted: bool) {
    let server = MockServer::start().await;
    let mut body: Value = serde_json::from_str(TIE).unwrap();
    body["answers"]["decision"]["choice"] = json!(choice);
    body["answers"]["decision"]["probabilities"]["A"] = json!(a);
    body["answers"]["decision"]["probabilities"]["B"] = json!(b);
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1)
        .mount(&server)
        .await;

    let result = client(&server)
        .decide(&two_option_request("Tea or coffee?"))
        .await;
    assert_eq!(result.is_ok(), accepted, "case: {name}, result: {result:?}");
}

#[tokio::test]
async fn transport_failure_is_value_free_and_is_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server_thread = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        read_request(&mut stream);
    });
    let client = JevClient::with_endpoint_and_timings(
        "sensitive-api-key",
        "sensitive-model",
        &format!("http://{address}/v1/systemone"),
        fast_timings(),
    )
    .unwrap();

    let error = client.decide(&request_with_context()).await.unwrap_err();
    assert_eq!(error, JevError::Transport);
    assert_eq!(format!("{error:?}"), "Transport");
    assert_eq!(error.to_string(), "Jev transport failed");
    server_thread.join().unwrap();
}

#[test]
fn validates_configuration_and_restricts_insecure_endpoints_to_literal_loopback() {
    for result in [
        JevClient::new("", "model"),
        JevClient::new("key", ""),
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "http://example.com/v1/systemone",
            fast_timings(),
        ),
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "http://localhost/v1/systemone",
            fast_timings(),
        ),
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "http://127.0.0.1/v1/systemone?redirect=https://evil.example",
            fast_timings(),
        ),
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "ftp://127.0.0.1/v1/systemone",
            fast_timings(),
        ),
    ] {
        assert!(matches!(result, Err(JevError::Configuration)));
    }

    assert!(
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "https://example.com/v1/systemone",
            fast_timings(),
        )
        .is_ok()
    );
    assert!(
        JevClient::with_endpoint_and_timings(
            "key",
            "model",
            "http://[::1]:1234/v1/systemone",
            fast_timings(),
        )
        .is_ok()
    );
}

#[test]
fn default_timings_match_the_bounded_production_contract() {
    assert_eq!(
        JevClientTimings::default(),
        JevClientTimings {
            connect_timeout: Duration::from_secs(3),
            attempt_timeout: Duration::from_secs(10),
            total_timeout: Duration::from_secs(30),
            retry_delays: [Duration::from_millis(500), Duration::from_secs(1)],
        }
    );
}

fn raw_delayed_body_server(body: Vec<u8>, delay: Duration) -> (String, thread::JoinHandle<()>) {
    raw_server(move |mut stream| {
        read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.flush().unwrap();
        thread::sleep(delay);
        let _ = stream.write_all(&body);
    })
}

fn raw_chunked_server(body: Vec<u8>) -> (String, thread::JoinHandle<()>) {
    raw_server(move |mut stream| {
        read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(&body).unwrap();
        stream.write_all(b"\r\n0\r\n\r\n").unwrap();
    })
}

fn raw_stalled_error_body_server(status: u16, stall: Duration) -> (String, thread::JoinHandle<()>) {
    raw_server(move |mut stream| {
        read_request(&mut stream);
        write!(
            stream,
            "HTTP/1.1 {status} Error\r\nContent-Type: text/plain\r\nContent-Length: 32\r\nConnection: close\r\n\r\n"
        )
        .unwrap();
        stream.flush().unwrap();
        thread::sleep(stall);
        let _ = stream.write_all(b"provider body must remain unread!!");
    })
}

fn raw_server(
    handler: impl FnOnce(std::net::TcpStream) + Send + 'static,
) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        handler(stream);
    });
    (format!("http://{address}/v1/systemone"), handle)
}

fn read_request(stream: &mut std::net::TcpStream) {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                request.extend_from_slice(&buffer[..count]);
                if let Some(headers_end) =
                    request.windows(4).position(|window| window == b"\r\n\r\n")
                {
                    let header_text = String::from_utf8_lossy(&request[..headers_end]);
                    let content_length = header_text
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= headers_end + 4 + content_length {
                        break;
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(error) => panic!("failed to read request: {error}"),
        }
    }
}
