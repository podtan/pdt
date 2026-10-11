//! REAL-BINARY IT GATE — auth-degraded filtered reads answer 503, never 200-empty
//! (issue 365be03f, production-gate promotion 2026-10-10).
//!
//! Field class being closed: when userinfo enrichment fails (IdP session died
//! while the JWT is not yet expired — payam F1, 2026-10-07 07:24Z), pep
//! swallows the HTTP error as non-fatal Ok, the principal evaluates as
//! zero-attribute, and visibility-filtered search/list answered 200-with-empty
//! — a lying absence indistinguishable from genuine no-data.
//!
//! This gate drives the REAL pdt binary (sqlite-backend) against an
//! in-process mock IdP whose userinfo endpoint can be flipped mid-flight:
//!   phase 1 (enriched)  — fresh sub enriches, canary asset visible: 200 WITH rows
//!   phase 2 (degraded)  — userinfo answers 400 invalid_token; a FRESH sub
//!                         (dodges the per-sub userinfo cache) gets:
//!                         search → 503 {"error":"auth_degraded","retryable":true}
//!                         list   → 503 same body
//!                         asset-GET → honest deny (never 200-with-content)
//!   phase 3 (recovered) — userinfo restored; another fresh sub sees 200 WITH rows
//!
//! Fresh-sub-per-phase is load-bearing: pep caches userinfo by `sub` until
//! token expiry, so a reused sub would never re-hit the failing endpoint.
//!
//! Run: cargo test --features sqlite-backend --test auth_degraded_it -- --nocapture
#![cfg(feature = "sqlite-backend")]

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

// Throwaway RSA-2048 key generated solely for these tests. Never used anywhere.
const PRIV_KEY: &str = include_str!("keys/test_rsa.pem");
const MODULUS_B64: &str = include_str!("keys/modulus_b64.txt");

const KID: &str = "test-key";
const GROUP: &str = "plt-agents";

// ───────────────────────────── mock IdP ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum UserinfoMode {
    Ok,
    Reject,
}

struct MockIdp {
    base: String,
    mode: Arc<RwLock<UserinfoMode>>,
    userinfo_hits: Arc<AtomicUsize>,
}

fn jwks_doc() -> String {
    format!(
        r#"{{"keys":[{{"kty":"RSA","kid":"{KID}","n":"{}","e":"AQAB","alg":"RS256"}}]}}"#,
        MODULUS_B64.trim()
    )
}

fn http_response(status_line: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

async fn spawn_mock_idp() -> MockIdp {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("mock IdP bind");
    let addr = listener.local_addr().expect("mock IdP addr");
    let base = format!("http://{addr}");

    let mode = Arc::new(RwLock::new(UserinfoMode::Ok));
    let userinfo_hits = Arc::new(AtomicUsize::new(0));

    let task_mode = Arc::clone(&mode);
    let task_hits = Arc::clone(&userinfo_hits);
    let task_base = base.clone();

    tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(x) => x,
                Err(_) => return,
            };
            let mode = Arc::clone(&task_mode);
            let hits = Arc::clone(&task_hits);
            let base = task_base.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = Vec::with_capacity(2048);
                let mut chunk = [0u8; 2048];
                loop {
                    let n = match stream.read(&mut chunk).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => n,
                    };
                    buf.extend_from_slice(&chunk[..n]);
                    if buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    if buf.len() > 32_768 {
                        return;
                    }
                }
                let req = String::from_utf8_lossy(&buf);
                let path = req.split_whitespace().nth(1).unwrap_or("/");

                let resp = if path.starts_with("/.well-known/openid-configuration") {
                    let body = format!(r#"{{"issuer":"{base}","jwks_uri":"{base}/jwks"}}"#);
                    http_response("200 OK", &body)
                } else if path.starts_with("/jwks") {
                    http_response("200 OK", &jwks_doc())
                } else if path.starts_with("/userinfo") {
                    hits.fetch_add(1, Ordering::SeqCst);
                    let m = *mode.read().expect("mode lock");
                    match m {
                        UserinfoMode::Ok => http_response(
                            "200 OK",
                            r#"{"sub":"ignored","groups":["plt-agents"],"role":"member"}"#,
                        ),
                        // The exact incident wire: Kanidm rejecting the dead
                        // session's token at userinfo (payam F1, 07:24:45Z).
                        UserinfoMode::Reject => {
                            http_response("400 Bad Request", r#"{"error":"invalid_token"}"#)
                        }
                    }
                } else {
                    http_response("404 Not Found", r#"{"error":"not found"}"#)
                };

                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });

    MockIdp {
        base,
        mode,
        userinfo_hits,
    }
}

impl MockIdp {
    fn set_mode(&self, m: UserinfoMode) {
        *self.mode.write().expect("mode lock") = m;
    }
}

// ─────────────────────────── real pdt binary ───────────────────────────

struct PdtProcess {
    child: tokio::process::Child,
    base: String,
    dir: PathBuf,
}

impl Drop for PdtProcess {
    fn drop(&mut self) {
        // Kill-guard: no stray state, ever (d4ff6605 law 3).
        let _ = self.child.start_kill();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn pdt_binary_path() -> PathBuf {
    if let Ok(p) = std::env::var("PDT_IT_BIN") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/debug/pdt")
}

async fn spawn_pdt(idp: &MockIdp) -> PdtProcess {
    let bin = pdt_binary_path();
    assert!(
        bin.exists(),
        "REAL-BINARY GATE: pdt binary missing at {} — build it first: \
         cargo build --features sqlite-backend --bin pdt (a skip must never look like a pass)",
        bin.display()
    );

    let dir = std::env::temp_dir().join(format!(
        "pdt-it-degraded-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(dir.join("instances")).expect("tmp dir");

    // Pre-pick a free port (bind + drop; poll-health covers the race).
    let port_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("port pick");
    let port = port_listener.local_addr().expect("port").port();
    drop(port_listener);

    let stdout_file = std::fs::File::create(dir.join("pdt-stdout.log")).expect("stdout log");
    let stderr_file = std::fs::File::create(dir.join("pdt-stderr.log")).expect("stderr log");

    let child = tokio::process::Command::new(&bin)
        .env("PDT_DB_BACKEND", "sqlite")
        .env("SQLITE_PATH", dir.join("root.db"))
        .env("PDT_INSTANCES_DIR", dir.join("instances"))
        .env("PDT_HOST", "127.0.0.1")
        .env("PDT_PORT", port.to_string())
        .env("AUTH_ENABLED", "true")
        .env("AUTH_ISSUER_URL", &idp.base)
        .env("AUTH_USERINFO_URL", format!("{}/userinfo", idp.base))
        .env("CEDAR_ENABLED", "true")
        .env("RUST_LOG", "info")
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file))
        .spawn()
        .expect("spawn real pdt binary");

    let base = format!("http://127.0.0.1:{port}");
    PdtProcess { child, base, dir }
}

// ───────────────────────────── helpers ─────────────────────────────

fn sign_token(issuer: &str, sub: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs();
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(KID.to_string());
    let claims = serde_json::json!({
        "sub": sub,
        "iss": issuer,
        "aud": "pdt-it",
        "iat": now,
        "exp": now + 600,
        // Kanidm-style: access tokens carry NO identity claims —
        // groups/role can only arrive via the userinfo merge.
    });
    let key = EncodingKey::from_rsa_pem(PRIV_KEY.as_bytes()).expect("valid test RSA PEM");
    encode(&header, &claims, &key).expect("sign test token")
}

async fn wait_health(base: &str) -> reqwest::Client {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client");
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(resp) = client.get(format!("{base}/health")).send().await {
            if resp.status().is_success() {
                return client;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "pdt never became healthy at {base} (poll-not-sleep law: bounded loop, then fail loud)"
        );
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

// ───────────────────────────── the gate ─────────────────────────────

#[tokio::test]
async fn degraded_enrichment_never_answers_200_empty_on_filtered_reads() {
    let idp = spawn_mock_idp().await;
    let pdt = spawn_pdt(&idp).await;
    let client = wait_health(&pdt.base).await;
    eprintln!("OBSERVED pdt up at {} (mock IdP {})", pdt.base, idp.base);

    let canary_token = "degradedcanary365be03f";
    let title = format!("canary {canary_token}");
    let auth = |sub: &str| format!("Bearer {}", sign_token(&idp.base, sub));

    // ── phase 1: ENRICHED — the control that makes the 503 meaningful ──
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("agent-enriched"))
        .json(&serde_json::json!({
            "title": title,
            "content": format!("canary body {canary_token} secret-marker-zqx"),
            "auth_context": {
                "visibility": "team",
                "owner_groups": [GROUP],
            },
        }))
        .send()
        .await
        .expect("create canary");
    let status = resp.status();
    assert!(
        status.is_success(),
        "enriched principal must be able to create the canary (got {status})"
    );
    let created: serde_json::Value = resp.json().await.expect("create body");
    let asset_id = created["_id"]
        .as_str()
        .expect("asset id (Asset serializes id as _id)")
        .to_string();
    eprintln!("OBSERVED canary created id={asset_id}");

    let resp = client
        .get(format!("{}/api/search", pdt.base))
        .query(&[("q", canary_token)])
        .header("Authorization", auth("agent-enriched"))
        .send()
        .await
        .expect("enriched search");
    assert_eq!(resp.status(), 200, "enriched search must be 200");
    let body: serde_json::Value = resp.json().await.expect("search body");
    let hit = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .any(|r| r["_id"].as_str() == Some(asset_id.as_str()));
    assert!(
        hit,
        "enriched control: canary must be VISIBLE in search (200 WITH rows)"
    );
    eprintln!("OBSERVED phase-1 enriched search 200 with canary visible");

    // ── phase 2: DEGRADED — the incident, reproduced on the wire ──
    idp.set_mode(UserinfoMode::Reject);

    let resp = client
        .get(format!("{}/api/search", pdt.base))
        .query(&[("q", canary_token)])
        .header("Authorization", auth("agent-degraded"))
        .send()
        .await
        .expect("degraded search");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("degraded search body");
    assert_eq!(
        status,
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "degraded search must be 503 — NEVER 200-empty (issue 365be03f)"
    );
    assert_eq!(
        body["error"].as_str(),
        Some("auth_degraded"),
        "contract marker missing: {body}"
    );
    assert_eq!(
        body["retryable"].as_bool(),
        Some(true),
        "retryable marker missing: {body}"
    );
    eprintln!("OBSERVED degraded search → 503 auth_degraded retryable=true");

    let resp = client
        .get(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("agent-degraded"))
        .send()
        .await
        .expect("degraded list");
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.expect("degraded list body");
    assert_eq!(
        status,
        reqwest::StatusCode::SERVICE_UNAVAILABLE,
        "degraded list must be 503 — NEVER 200-empty"
    );
    assert_eq!(body["error"].as_str(), Some("auth_degraded"));
    eprintln!("OBSERVED degraded list → 503 auth_degraded");

    // Honesty preserved on the single-asset surface: an honest deny, never a
    // 200 carrying content the principal's (unverifiable) attributes gate.
    let resp = client
        .get(format!("{}/api/assets/{asset_id}", pdt.base))
        .header("Authorization", auth("agent-degraded"))
        .send()
        .await
        .expect("degraded asset get");
    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    assert_ne!(status, 200, "degraded asset-GET must not answer 200");
    assert!(
        status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND,
        "degraded asset-GET must be an honest deny (403/404), got {status}"
    );
    assert!(
        !body_text.contains("secret-marker-zqx"),
        "content must not leak on the deny path"
    );
    eprintln!("OBSERVED degraded asset-GET → {status} (honest deny, no content leak)");

    // The mock IdP was genuinely exercised per fresh sub (cache-dodge proof):
    // enriched, degraded, and recovery subs each had to reach userinfo.
    let hits = idp.userinfo_hits.load(Ordering::SeqCst);
    assert!(
        hits >= 3,
        "userinfo must have been hit at least once per fresh sub (got {hits}) — \
         otherwise the degradation was masked by pep's userinfo cache"
    );

    // ── phase 3: RECOVERED — the window closes honestly ──
    idp.set_mode(UserinfoMode::Ok);
    let resp = client
        .get(format!("{}/api/search", pdt.base))
        .query(&[("q", canary_token)])
        .header("Authorization", auth("agent-recovered"))
        .send()
        .await
        .expect("recovered search");
    assert_eq!(resp.status(), 200, "recovered search must be 200");
    let body: serde_json::Value = resp.json().await.expect("recovered body");
    let hit = body["data"]
        .as_array()
        .expect("data array")
        .iter()
        .any(|r| r["_id"].as_str() == Some(asset_id.as_str()));
    assert!(
        hit,
        "recovered control: canary visible again after userinfo recovery"
    );
    eprintln!("OBSERVED phase-3 recovered search 200 with canary visible — window closed");

    eprintln!(
        "OBSERVED receipts: userinfo_hits={} , canary={asset_id}",
        idp.userinfo_hits.load(Ordering::SeqCst)
    );
}
