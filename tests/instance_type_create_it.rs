//! REAL-BINARY IT GATE — instance-type Create denied PRE-WRITE for non-admins
//! (kavosh #222, CPO ruling ENFORCE, Rahnama patch-class acceptance).
//!
//! Field class being closed: the create path was the ONLY write handler
//! without a Cedar consult — the instance-type forbid (rbac.cedar) evaluates
//! resource.asset_type, which only exists on stored assets, so it
//! dead-lettered on create and a non-admin could POST an instance-type asset
//! directly (wire proof: 200 created, rollback DELETE 403).
//!
//! This gate drives the REAL pdt binary (sqlite-backend) against the
//! in-process mock IdP (the auth_degraded_it harness pattern):
//!   phase 1 (non-admin) — instance-type create → 403 PRE-WRITE (the store
//!                         count unchanged — nothing landed); ordinary
//!                         document create → 200 (any-authenticated Create
//!                         stays, ruling 9e32f9ba untouched)
//!   phase 2 (admin)     — instance-type create → 200 (the admin permit)
//!
//! Fresh-sub-per-phase is load-bearing: pep caches userinfo by `sub` until
//! token expiry, so a reused sub would never re-hit the flipped endpoint.
//!
//! Run: cargo test --features sqlite-backend --test instance_type_create_it -- --nocapture
#![cfg(feature = "sqlite-backend")]

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

// Throwaway RSA-2048 key generated solely for these tests. Never used anywhere.
const PRIV_KEY: &str = include_str!("keys/test_rsa.pem");
const MODULUS_B64: &str = include_str!("keys/modulus_b64.txt");

const KID: &str = "test-key";

// ───────────────────────────── mock IdP ─────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum UserinfoMode {
    /// 200 with groups + role=member — the NON-ADMIN principal shape.
    OkMember,
    /// 200 with role=admin — the enriched-admin control (token-claim path).
    OkAdmin,
}

struct MockIdp {
    base: String,
    mode: Arc<RwLock<UserinfoMode>>,
    #[allow(dead_code)]
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

    let mode = Arc::new(RwLock::new(UserinfoMode::OkMember));
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
                    hits.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let m = *mode.read().expect("mode lock");
                    match m {
                        UserinfoMode::OkMember => http_response(
                            "200 OK",
                            r#"{"sub":"ignored","groups":["plt-agents"],"role":"member"}"#,
                        ),
                        UserinfoMode::OkAdmin => http_response(
                            "200 OK",
                            r#"{"sub":"ignored","groups":["plt-agents"],"role":"admin"}"#,
                        ),
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
        "pdt-it-instance-type-{}-{}",
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

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.env("PDT_DB_BACKEND", "sqlite")
        .env("SQLITE_PATH", dir.join("root.db"))
        .env("PDT_INSTANCES_DIR", dir.join("instances"))
        .env("PDT_HOST", "127.0.0.1")
        .env("PDT_PORT", port.to_string())
        .env("AUTH_ENABLED", "true")
        .env("AUTH_ISSUER_URL", &idp.base)
        .env("AUTH_USERINFO_URL", format!("{}/userinfo", idp.base))
        .env("CEDAR_ENABLED", "true")
        .env("RUST_LOG", "info");
    let child = cmd
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

async fn list_count(client: &reqwest::Client, base: &str, auth: &str) -> usize {
    let resp = client
        .get(format!("{base}/api/assets"))
        .header("Authorization", auth)
        .send()
        .await
        .expect("list assets");
    assert_eq!(resp.status(), 200, "list must be 200 for the caller");
    let body: serde_json::Value = resp.json().await.expect("list body");
    body["data"].as_array().expect("data array").len()
}

// ───────────────────────────── the gate ─────────────────────────────

#[tokio::test]
async fn instance_type_create_denied_pre_write_for_non_admin() {
    let idp = spawn_mock_idp().await;
    let pdt = spawn_pdt(&idp).await;
    let client = wait_health(&pdt.base).await;
    eprintln!("OBSERVED pdt up at {} (mock IdP {})", pdt.base, idp.base);

    let auth = |sub: &str| format!("Bearer {}", sign_token(&idp.base, sub));

    // ── phase 1: NON-ADMIN (role=member via the userinfo merge) ──
    let before = list_count(&client, &pdt.base, &auth("agent-member")).await;
    eprintln!("OBSERVED pre-state list count = {before}");

    // THE CLASS: instance-type create by a non-admin → 403 PRE-WRITE.
    // The payload carries NO auth_context — the guarded birth context the
    // probe carries is what the store WOULD have persisted; the consult
    // must fire on it (a probe gated on the raw request would skip the
    // consult exactly when the payload omits the band).
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("agent-member"))
        .json(&serde_json::json!({
            "title": "rogue instance-type (kavosh #222)",
            "content": "must be denied pre-write",
            "tags": [{"category": "asset_type", "value": "instance-type"}],
        }))
        .send()
        .await
        .expect("instance-type create");
    let status = resp.status();
    assert_eq!(
        status,
        reqwest::StatusCode::FORBIDDEN,
        "non-admin instance-type create must be 403 PRE-WRITE (got {status})"
    );
    let body: serde_json::Value = resp.json().await.expect("deny body");
    assert_eq!(body["code"].as_str(), Some("FORBIDDEN"), "{body}");
    assert!(
        body["message"]
            .as_str()
            .unwrap_or("")
            .contains("'Create' permission"),
        "the deny must be a Create-permission denial, got: {body}"
    );
    eprintln!("OBSERVED instance-type create → 403 pre-write: {body}");

    // PRE-WRITE proof: the store count is UNCHANGED — nothing landed.
    let after_deny = list_count(&client, &pdt.base, &auth("agent-member")).await;
    assert_eq!(
        after_deny, before,
        "the deny must be PRE-WRITE: the store count must be unchanged"
    );

    // CONTROL (ruling 9e32f9ba untouched): ordinary-asset create stays
    // any-authenticated — 200, and visible to its cohort.
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", &auth("agent-member"))
        .json(&serde_json::json!({
            "title": "ordinary document (control)",
            "content": "any-authenticated Create stays",
            "tags": [{"category": "asset_type", "value": "document"}],
        }))
        .send()
        .await
        .expect("ordinary create");
    let status = resp.status();
    assert!(
        status.is_success(),
        "ordinary-asset create stays any-authenticated (got {status})"
    );
    let after_control = list_count(&client, &pdt.base, &auth("agent-member")).await;
    assert_eq!(
        after_control,
        before + 1,
        "the ordinary create must land exactly one asset"
    );
    eprintln!("OBSERVED ordinary document create → 200, count {before}→{after_control}");

    // ── phase 2: ADMIN (role=admin via the userinfo merge) — the permit ──
    idp.set_mode(UserinfoMode::OkAdmin);
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header(
            "Authorization",
            format!("Bearer {}", sign_token(&idp.base, "agent-admin")),
        )
        .json(&serde_json::json!({
            "title": "admin instance-type (permit)",
            "content": "the admin permit keeps working",
            "tags": [{"category": "asset_type", "value": "instance-type"}],
        }))
        .send()
        .await
        .expect("admin instance-type create");
    let status = resp.status();
    assert!(
        status.is_success(),
        "admin instance-type create must keep working (got {status})"
    );
    let created: serde_json::Value = resp.json().await.expect("admin create body");
    let asset_type = created["tags"]
        .as_array()
        .expect("tags array")
        .iter()
        .find(|t| t["category"] == "asset_type")
        .and_then(|t| t["value"].as_str())
        .expect("asset_type tag");
    assert_eq!(asset_type, "instance-type", "the stored tag must survive");
    eprintln!("OBSERVED admin instance-type create → 200 (the admin permit intact)");
}
