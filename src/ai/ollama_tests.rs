use super::ollama::*;
use crate::testutil::{http_response, Behavior, FakeHttp};
use serde_json::Value;
use std::net::TcpListener;
use std::time::{Duration, Instant};

fn settings(port: u16) -> Settings {
    Settings {
        model: "qwen2.5:1.5b".into(),
        port,
        timeout: Duration::from_secs(5),
        unload_after: false,
    }
}

fn server(script: Vec<Behavior>) -> (FakeHttp, u16) {
    let s = FakeHttp::serve(script);
    let port = s.addr.port();
    (s, port)
}

fn ok(json: &str) -> Behavior {
    Behavior::Raw(http_response(200, json))
}

fn failure(status: u16, json: &str) -> Behavior {
    Behavior::Raw(http_response(status, json))
}

fn body_of(request: &str) -> Value {
    let (_, body) = request.split_once("\r\n\r\n").unwrap();
    serde_json::from_str(body).unwrap()
}

#[test]
fn a_successful_call_returns_the_models_text() {
    let (srv, port) = server(vec![ok(r#"{"model":"m","response":"{\"statements\":[]}","done":true}"#)]);
    let text = Ollama::new(settings(port)).unwrap().complete("PROMPT").unwrap();
    assert_eq!(text, r#"{"statements":[]}"#);
    assert_eq!(srv.requests().len(), 1);
}

#[test]
fn the_request_is_deterministic_bounded_and_constrained() {
    let (srv, port) = server(vec![ok(r#"{"response":"x","done":true}"#)]);
    let prompt = "p".repeat(4000);
    Ollama::new(settings(port)).unwrap().complete(&prompt).unwrap();

    let req = &srv.requests()[0];
    assert!(req.starts_with("POST /api/generate HTTP/1.1\r\n"));
    assert!(req.contains(&format!("Host: 127.0.0.1:{port}")));

    let b = body_of(req);
    assert_eq!(b["model"], "qwen2.5:1.5b");
    assert_eq!(b["prompt"], prompt.as_str(), "the prompt must be sent unchanged");
    assert_eq!(b["stream"], false);
    assert_eq!(b["options"]["temperature"], 0);
    assert_eq!(b["options"]["seed"], 0);
    assert_eq!(b["options"]["num_predict"], NUM_PREDICT);
    assert_eq!(b["options"]["num_ctx"], context_for(1000));
    assert!(b.get("keep_alive").is_none(), "the model stays loaded unless asked to unload");
    // The reply is constrained to the statements shape.
    let f = &b["format"];
    assert_eq!(f["type"], "object");
    assert_eq!(f["required"][0], "statements");
    assert_eq!(f["properties"]["statements"]["items"]["required"], serde_json::json!(["text", "cites"]));
}

#[test]
fn unloading_after_use_is_requested_only_when_configured() {
    let (srv, port) = server(vec![ok(r#"{"response":"x","done":true}"#)]);
    let mut s = settings(port);
    s.unload_after = true;
    Ollama::new(s).unwrap().complete("p").unwrap();
    assert_eq!(body_of(&srv.requests()[0])["keep_alive"], 0);
}

#[test]
fn the_context_window_always_fits_the_prompt_and_the_reply() {
    assert_eq!(context_for(0), 4096);
    assert_eq!(context_for(500), 4096);
    assert_eq!(context_for(6000), 8192);
    assert_eq!(context_for(100_000), 16384);
    for tokens in (0..9000).step_by(250) {
        let ctx = context_for(tokens);
        assert_eq!(ctx % 1024, 0);
        assert!((4096..=16384).contains(&ctx));
        if ctx < 16384 {
            assert!(ctx >= tokens + NUM_PREDICT, "{tokens} tokens do not fit in {ctx}");
        }
    }
}

#[test]
fn the_largest_possible_payload_fits_in_the_window() {
    use super::payload::MAX_PAYLOAD_BYTES;
    let tokens = (MAX_PAYLOAD_BYTES + 3) / 4;
    assert!(context_for(tokens) >= tokens + NUM_PREDICT, "the biggest payload must not be cut off by the model's window");
}

#[test]
fn older_servers_get_a_second_request_in_plain_json_mode() {
    let (srv, port) = server(vec![
        failure(400, r#"{"error":"invalid format: expected \"json\""}"#),
        ok(r#"{"response":"fallback worked","done":true}"#),
    ]);
    let text = Ollama::new(settings(port)).unwrap().complete("p").unwrap();
    assert_eq!(text, "fallback worked");
    let reqs = srv.requests();
    assert_eq!(reqs.len(), 2);
    assert!(body_of(&reqs[0])["format"].is_object());
    assert_eq!(body_of(&reqs[1])["format"], "json");
}

#[test]
fn other_bad_requests_are_not_retried() {
    let (srv, port) = server(vec![failure(400, r#"{"error":"prompt is empty"}"#), ok(r#"{"response":"never","done":true}"#)]);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
    assert!(matches!(e, BackendError::Failed(ref m) if m.contains("status 400") && m.contains("prompt is empty")), "{e}");
    assert_eq!(srv.requests().len(), 1);
}

#[test]
fn a_missing_model_says_how_to_install_it() {
    let (_srv, port) = server(vec![failure(404, r#"{"error":"model 'qwen2.5:1.5b' not found, try pulling it first"}"#)]);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
    assert_eq!(e, BackendError::ModelMissing("qwen2.5:1.5b".into()));
    assert!(e.to_string().contains("ollama pull qwen2.5:1.5b"));
}

#[test]
fn a_404_that_is_not_about_the_model_is_a_plain_failure() {
    let (_srv, port) = server(vec![failure(404, r#"{"error":"unknown route"}"#)]);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
    assert!(matches!(e, BackendError::Failed(_)), "{e}");
}

#[test]
fn a_server_that_is_not_running_gives_actionable_advice() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
    assert_eq!(e, BackendError::NotRunning(port));
    let msg = e.to_string();
    assert!(msg.contains(&format!("127.0.0.1:{port}")) && msg.contains("ollama serve") && msg.contains("snap start ollama"), "{msg}");
}

#[test]
fn a_slow_model_times_out_with_a_hint() {
    let (_srv, port) = server(vec![Behavior::Hang]);
    let mut s = settings(port);
    s.timeout = Duration::from_millis(300);
    let start = Instant::now();
    let e = Ollama::new(s).unwrap().complete("p").unwrap_err();
    assert!(matches!(e, BackendError::Timeout(_)), "{e}");
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn an_answer_that_hit_the_length_limit_is_reported_not_used() {
    let (_srv, port) = server(vec![ok(r#"{"response":"{\"statements\":[{\"text\":\"cut","done":true,"done_reason":"length"}"#)]);
    assert_eq!(Ollama::new(settings(port)).unwrap().complete("p").unwrap_err(), BackendError::Truncated);
}

#[test]
fn unusable_replies_are_errors_not_panics() {
    for (name, body) in [
        ("not json", "<html>proxy error</html>"),
        ("no response field", r#"{"done":true}"#),
        ("response is not text", r#"{"response":42}"#),
        ("an array", "[]"),
        ("empty", ""),
    ] {
        let (_srv, port) = server(vec![ok(body)]);
        let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
        assert!(matches!(e, BackendError::BadResponse(_)), "{name}: {e}");
    }
}

#[test]
fn an_oversized_reply_is_rejected() {
    let big = format!(r#"{{"response":"{}","done":true}}"#, "x".repeat(3 * 1024 * 1024));
    let (_srv, port) = server(vec![ok(&big)]);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err();
    assert!(matches!(e, BackendError::BadResponse(ref m) if m.contains("too large")), "{e}");
}

#[test]
fn credentials_in_error_text_are_redacted_and_long_text_is_cut() {
    let secret = "ghp_abcdefghijklmnopqrstuvwxyz0123456789";
    let (_srv, port) = server(vec![failure(500, &format!(r#"{{"error":"failed at https://u:{secret}@host/x {}"}}"#, "y".repeat(1000)))]);
    let e = Ollama::new(settings(port)).unwrap().complete("p").unwrap_err().to_string();
    assert!(!e.contains(secret) && !e.contains("abcdefghijklmnop"), "{e}");
    assert!(e.chars().count() < 400, "{}", e.chars().count());
}

#[test]
fn invalid_model_names_are_refused_before_anything_is_sent() {
    for bad in ["", "a b", "x;rm", "../x", "a\nb", "-flag", ".hidden", "/abs", ":tag", "a$(b)", "a..b", &"m".repeat(101), "caf\u{e9}"] {
        let mut s = settings(1);
        s.model = bad.into();
        assert!(matches!(Ollama::new(s), Err(BackendError::InvalidModel(_))), "{bad:?}");
    }
    for good in ["qwen2.5:1.5b", "llama3.2:1b", "library/llama3.2:1b", "hf.co/user/model:Q4_K_M", "phi3", "a-b_c.d"] {
        assert!(valid_model_name(good), "{good}");
    }
}

#[test]
fn the_prompt_is_json_escaped_so_it_cannot_alter_the_request() {
    let (srv, port) = server(vec![ok(r#"{"response":"x","done":true}"#)]);
    let nasty = "line1\n\"quoted\"\\back\u{0}\u{1f600}\"},\"stream\":true,\"x\":\"";
    Ollama::new(settings(port)).unwrap().complete(nasty).unwrap();
    let b = body_of(&srv.requests()[0]);
    assert_eq!(b["prompt"], nasty);
    assert_eq!(b["stream"], false, "the prompt must not be able to change other fields");
}

#[test]
fn only_the_loopback_port_is_ever_used() {
    let (srv, port) = server(vec![ok(r#"{"response":"x","done":true}"#)]);
    Ollama::new(settings(port)).unwrap().complete("p").unwrap();
    assert!(srv.addr.ip().is_loopback());
}
