use super::http::{post_json, HttpError};
use crate::testutil::{http_response, Behavior, FakeHttp};
use std::net::{SocketAddr, TcpListener};
use std::time::{Duration, Instant};

const T: Duration = Duration::from_secs(5);
const BIG: usize = 1 << 20;

fn ok_body(status: u16, body: &str) -> Vec<u8> {
    http_response(status, body)
}

#[test]
fn a_request_is_one_well_formed_post_with_the_exact_body() {
    let server = FakeHttp::serve(vec![Behavior::Raw(ok_body(200, r#"{"ok":true}"#))]);
    let body = r#"{"model":"m","prompt":"caf\u00e9 \u4e2d"}"#;
    let r = post_json(server.addr, "/api/generate", body, T, BIG).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(r.body, br#"{"ok":true}"#);

    let req = &server.requests()[0];
    let (head, sent_body) = req.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /api/generate HTTP/1.1\r\n"), "{head}");
    assert!(head.contains(&format!("Host: {}", server.addr)));
    assert!(head.contains("Content-Type: application/json"));
    assert!(head.contains("Connection: close"));
    assert!(head.contains(&format!("Content-Length: {}", body.len())));
    assert_eq!(sent_body, body);
}

#[test]
fn content_length_counts_bytes_not_characters() {
    let server = FakeHttp::serve(vec![Behavior::Raw(ok_body(200, "{}"))]);
    let body = "{\"p\":\"caf\u{e9} \u{4e2d}\u{6587} \u{1f600}\"}";
    assert_ne!(body.len(), body.chars().count());
    post_json(server.addr, "/x", body, T, BIG).unwrap();
    let req = &server.requests()[0];
    assert!(req.contains(&format!("Content-Length: {}\r\n", body.len())));
    assert!(req.ends_with(body));
}

#[test]
fn error_statuses_are_returned_not_hidden() {
    for code in [400u16, 404, 500, 503] {
        let server = FakeHttp::serve(vec![Behavior::Raw(ok_body(code, r#"{"error":"nope"}"#))]);
        let r = post_json(server.addr, "/x", "{}", T, BIG).unwrap();
        assert_eq!(r.status, code);
        assert_eq!(r.body, br#"{"error":"nope"}"#);
    }
}

#[test]
fn chunked_responses_are_decoded_including_extensions() {
    let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\n7\r\n, world\r\n0\r\n\r\n".to_vec();
    let server = FakeHttp::serve(vec![Behavior::Raw(raw)]);
    let r = post_json(server.addr, "/x", "{}", T, BIG).unwrap();
    assert_eq!(r.body, b"hello, world");
}

#[test]
fn a_response_with_no_length_is_read_until_the_connection_closes() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"until\":\"close\"}".to_vec();
    let server = FakeHttp::serve(vec![Behavior::Raw(raw)]);
    let r = post_json(server.addr, "/x", "{}", T, BIG).unwrap();
    assert_eq!(r.body, br#"{"until":"close"}"#);
}

#[test]
fn headers_are_case_insensitive() {
    let raw = b"HTTP/1.1 200 OK\r\ncontent-LENGTH: 2\r\n\r\nhiEXTRA".to_vec();
    let server = FakeHttp::serve(vec![Behavior::Raw(raw)]);
    assert_eq!(post_json(server.addr, "/x", "{}", T, BIG).unwrap().body, b"hi");
}

#[test]
fn a_slowly_arriving_response_within_the_deadline_is_accepted() {
    let body = ok_body(200, r#"{"slow":true}"#);
    let server = FakeHttp::serve(vec![Behavior::Dribble(body, 7, Duration::from_millis(20))]);
    let r = post_json(server.addr, "/x", "{}", T, BIG).unwrap();
    assert_eq!(r.body, br#"{"slow":true}"#);
}

#[test]
fn a_server_that_never_answers_hits_the_deadline() {
    let server = FakeHttp::serve(vec![Behavior::Hang]);
    let start = Instant::now();
    let e = post_json(server.addr, "/x", "{}", Duration::from_millis(300), BIG).unwrap_err();
    assert!(matches!(e, HttpError::Timeout), "{e:?}");
    assert!(start.elapsed() < Duration::from_secs(3), "took {:?}", start.elapsed());
}

#[test]
fn a_response_that_trickles_forever_is_cut_off_by_the_overall_deadline() {
    let endless = ok_body(200, &"x".repeat(5000));
    let server = FakeHttp::serve(vec![Behavior::Dribble(endless, 1, Duration::from_millis(40))]);
    let start = Instant::now();
    let e = post_json(server.addr, "/x", "{}", Duration::from_millis(400), BIG).unwrap_err();
    assert!(matches!(e, HttpError::Timeout), "{e:?}");
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn oversized_responses_are_rejected_in_every_framing() {
    let big = "x".repeat(10_000);
    let by_length = FakeHttp::serve(vec![Behavior::Raw(ok_body(200, &big))]);
    assert!(matches!(post_json(by_length.addr, "/x", "{}", T, 5_000), Err(HttpError::TooLarge)));

    let chunked = format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{big}\r\n0\r\n\r\n", big.len());
    let by_chunks = FakeHttp::serve(vec![Behavior::Raw(chunked.into_bytes())]);
    assert!(matches!(post_json(by_chunks.addr, "/x", "{}", T, 5_000), Err(HttpError::TooLarge)));

    let until_close = format!("HTTP/1.1 200 OK\r\n\r\n{big}");
    let by_close = FakeHttp::serve(vec![Behavior::Raw(until_close.into_bytes())]);
    assert!(matches!(post_json(by_close.addr, "/x", "{}", T, 5_000), Err(HttpError::TooLarge)));
}

#[test]
fn malformed_responses_are_errors_not_panics() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("garbage", b"not http at all".to_vec()),
        ("no header end", b"HTTP/1.1 200 OK\r\nContent-Length: 2".to_vec()),
        ("bad version", b"FTP/1.0 200 OK\r\n\r\nhi".to_vec()),
        ("bad status", b"HTTP/1.1 abc OK\r\n\r\nhi".to_vec()),
        ("status out of range", b"HTTP/1.1 999 X\r\n\r\nhi".to_vec()),
        ("bad length", b"HTTP/1.1 200 OK\r\nContent-Length: ten\r\n\r\nhi".to_vec()),
        ("short body", b"HTTP/1.1 200 OK\r\nContent-Length: 50\r\n\r\nhi".to_vec()),
        ("bad chunk size", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\nhi\r\n0\r\n\r\n".to_vec()),
        ("truncated chunk", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\nhi".to_vec()),
        ("chunk without CRLF", b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhiXX0\r\n\r\n".to_vec()),
        ("empty", Vec::new()),
        ("binary headers", b"HTTP/1.1 200 OK\r\nX: \xff\xfe\r\n\r\nhi".to_vec()),
    ];
    for (name, raw) in cases {
        let server = FakeHttp::serve(vec![Behavior::Raw(raw)]);
        let r = post_json(server.addr, "/x", "{}", T, BIG);
        assert!(matches!(r, Err(HttpError::Malformed(_))), "{name}: {r:?}");
    }
}

#[test]
fn a_refused_connection_is_a_connect_error() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let e = post_json(addr, "/x", "{}", T, BIG).unwrap_err();
    assert!(matches!(e, HttpError::Connect(_)), "{e:?}");
}

#[test]
fn only_loopback_addresses_are_ever_contacted() {
    for addr in ["192.0.2.1:80", "10.0.0.5:11434", "8.8.8.8:443", "[2001:db8::1]:80"] {
        let a: SocketAddr = addr.parse().unwrap();
        let start = Instant::now();
        let e = post_json(a, "/x", "{}", T, BIG).unwrap_err();
        assert!(matches!(e, HttpError::NotLoopback), "{addr}: {e:?}");
        assert!(start.elapsed() < Duration::from_millis(500), "must refuse without connecting");
    }
    let v6: SocketAddr = "[::1]:9".parse().unwrap();
    assert!(!matches!(post_json(v6, "/x", "{}", T, BIG), Err(HttpError::NotLoopback)));
}

#[test]
fn paths_that_could_alter_the_request_are_refused() {
    let server = FakeHttp::serve(vec![Behavior::Raw(ok_body(200, "{}"))]);
    for path in ["api/generate", "", "/a b", "/a\r\nHost: evil", "/a\nX: y", "/a?x=1", "/a#f", "/\u{e9}", &format!("/{}", "a".repeat(300))] {
        let e = post_json(server.addr, path, "{}", T, BIG).unwrap_err();
        assert!(matches!(e, HttpError::InvalidRequest(_)), "{path:?}: {e:?}");
    }
    assert!(server.requests().is_empty(), "nothing may be sent for a refused path");
}

#[test]
fn an_http_error_describes_itself_without_hiding_the_detail() {
    assert_eq!(HttpError::Timeout.to_string(), "timed out");
    assert!(HttpError::InvalidRequest("path \"x\" is not allowed".into()).to_string().contains("path \"x\" is not allowed"));
    let e = HttpError::Connect(std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused"));
    assert!(e.to_string().contains("refused"));
}
