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
//!   phase 2a (dead token)  — userinfo 400 invalid_token (terminal) → honest
//!                          401-with-retry; a reactive refresh cures it
//!   phase 2b (degraded)    — userinfo 503 (transient) → a FRESH sub
//!                          (dodges the per-sub userinfo cache) gets:
//!                          search → 503 {"error":"auth_degraded","retryable":true}
//!                          list   → 503 same body
//!                          asset-GET → fail-closed pre-handler, never 200
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
    /// 400 invalid_token — terminal for the token (pep: retryable=false) →
    /// expect the b82a1925 401-with-retry contract; a reactive refresh cures it.
    Reject400,
    /// 503 — transient upstream (pep: retryable=true) → expect the exact
    /// 503 {"error":"auth_degraded","retryable":true} contract (365be03f).
    Reject503,
    /// 200 with NO groups/role — the exchanged-token shape (RFC 8693, the
    /// 6c8efe55 class): the IdP answers userinfo fine but the principal has
    /// no group/role material. Enrichment lands Ok-but-empty → Degraded
    /// marker; the maintenance admin gate must fall back to the registry
    /// (CPO precondition 1, routing 0ae73f46).
    Empty200,
    /// 200 with role=admin — the enriched-admin control (token-claim path).
    OkAdmin,
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
                        // The exact incident wire (payam F1, 07:24:45Z): the IdP
                        // rejecting the dead session's token at userinfo.
                        UserinfoMode::Reject400 => {
                            http_response("400 Bad Request", r#"{"error":"invalid_token"}"#)
                        }
                        // Transient upstream failure — the retryable flavor.
                        UserinfoMode::Reject503 => http_response(
                            "503 Service Unavailable",
                            r#"{"error":"temporarily_unavailable"}"#,
                        ),
                        // The exchanged-token shape: userinfo answers fine but
                        // carries NO identity material (6c8efe55 class).
                        UserinfoMode::Empty200 => {
                            http_response("200 OK", r#"{"sub":"ignored"}"#)
                        }
                        // The enriched-admin control: role=admin in claims.
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

    // ── phase 2a: DEAD-TOKEN flavor (userinfo 400 invalid_token) ──
    // Terminal for this token → honest 401-with-retry (b82a1925 contract).
    // A reactive refresh cures it — never a 200 of any kind.
    idp.set_mode(UserinfoMode::Reject400);

    let resp = client
        .get(format!("{}/api/search", pdt.base))
        .query(&[("q", canary_token)])
        .header("Authorization", auth("agent-stale"))
        .send()
        .await
        .expect("stale-token search");
    let status = resp.status();
    assert_eq!(
        status,
        reqwest::StatusCode::UNAUTHORIZED,
        "dead-token search must 401-with-retry — NEVER 200 of any kind"
    );
    let www = resp
        .headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(
        www.starts_with("Bearer error=\"invalid_token\""),
        "401 must carry the WWW-Authenticate retry signal, got: {www}"
    );
    let body: serde_json::Value = resp.json().await.expect("stale-token body");
    assert_eq!(body["error"].as_str(), Some("token_expired"), "{body}");
    assert_eq!(body["retryable"].as_bool(), Some(true), "{body}");
    eprintln!("OBSERVED dead-token (400) search → 401 token_expired retryable + challenge");

    // ── phase 2b: TRANSIENT flavor (userinfo 503) — the 503 contract ──
    idp.set_mode(UserinfoMode::Reject503);

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
    eprintln!("OBSERVED degraded (503) search → 503 auth_degraded retryable=true");

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

    // Under pep 0.5.8 fail-closed, the middleware answers BEFORE any handler
    // runs — asset-GET rides the same 503 (louder than the old Cedar deny).
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
        status == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || status == reqwest::StatusCode::UNAUTHORIZED,
        "degraded asset-GET must be 503/401 (fail-closed pre-handler), got {status}"
    );
    assert!(
        !body_text.contains("secret-marker-zqx"),
        "content must not leak on the deny path"
    );
    eprintln!("OBSERVED degraded asset-GET → {status} (fail-closed, no content leak)");

    // The mock IdP was genuinely exercised per fresh sub (cache-dodge proof):
    // enriched, stale, degraded, and recovery subs each had to reach userinfo.
    let hits = idp.userinfo_hits.load(Ordering::SeqCst);
    assert!(
        hits >= 4,
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

// ───────────────────────── born-empty guard (0e583f06) ─────────────────────────
//
// GUARD IT: an asset created with NO auth_context must be born with a real
// band — the caller's cohort (team + caller groups) — never the black-hole
// visibility="" state that matches no View permit except admin/creator.
// Attestation: a DIFFERENT sub in the same cohort Views the unstamped create
// → 200. Before the guard this was the born-mask class (assignees invisible
// to themselves; 7 manual heals on 2026-10-20 alone).
#[tokio::test]
async fn born_empty_create_defaults_to_cohort_band_and_is_visible_to_cohort() {
    let idp = spawn_mock_idp().await;
    let pdt = spawn_pdt(&idp).await;
    let client = wait_health(&pdt.base).await;
    eprintln!("OBSERVED pdt up at {} (mock IdP {})", pdt.base, idp.base);

    let auth = |sub: &str| format!("Bearer {}", sign_token(&idp.base, sub));

    // ── phase 1: creator creates with NO auth_context ──
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("creator-alice"))
        .json(&serde_json::json!({
            "title": "born-empty guard canary 0e583f06",
            "content": "unstamped create — must be born cohort-banded",
        }))
        .send()
        .await
        .expect("create unstamped");
    assert!(
        resp.status().is_success(),
        "unstamped create must succeed (guard defaults, never refuses a cohort caller) — got {}",
        resp.status()
    );
    let created: serde_json::Value = resp.json().await.expect("create body");
    let asset_id = created["_id"]
        .as_str()
        .expect("asset id (Asset serializes id as _id)")
        .to_string();
    let stored_visibility = created["auth_context"]["visibility"]
        .as_str()
        .expect("auth_context.visibility present after guard");
    let stored_groups = created["auth_context"]["owner_groups"]
        .as_array()
        .expect("owner_groups array after guard");
    eprintln!(
        "OBSERVED unstamped create born visibility={stored_visibility} owner_groups={stored_groups:?}"
    );
    assert_eq!(
        stored_visibility, "team",
        "guard must default an empty band to the caller's cohort (team)"
    );
    assert!(
        stored_groups.iter().any(|g| g.as_str() == Some(GROUP)),
        "guard must fill owner_groups from the caller's groups claim"
    );

    // ── phase 2: a DIFFERENT sub in the same cohort Views the asset → 200 ──
    // The born-mask class: before the guard this asset carried visibility=""
    // and matched no View permit for bob (different sub, not the creator).
    let resp = client
        .get(format!("{}/api/assets/{asset_id}", pdt.base))
        .header("Authorization", auth("assignee-bob"))
        .send()
        .await
        .expect("cohort view");
    assert_eq!(
        resp.status(),
        200,
        "cohort member (different sub, same group) must View the unstamped create — the born-mask is dead"
    );
    eprintln!("OBSERVED cohort member bob (different sub) Views the unstamped create → 200");

    // ── phase 3: explicit empty-visibility + owner_groups normalizes to team ──
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("creator-alice"))
        .json(&serde_json::json!({
            "title": "empty-visibility normalize canary 0e583f06",
            "auth_context": {
                "visibility": "",
                "owner_groups": [GROUP],
            },
        }))
        .send()
        .await
        .expect("create empty-visibility");
    assert!(
        resp.status().is_success(),
        "empty-visibility create must succeed (normalized, never refused)"
    );
    let created: serde_json::Value = resp.json().await.expect("create body");
    assert_eq!(
        created["auth_context"]["visibility"].as_str(),
        Some("team"),
        "empty visibility with owner_groups must normalize to team"
    );
    assert_eq!(
        created["auth_context"]["owner_groups"][0].as_str(),
        Some(GROUP),
        "caller-provided owner_groups must be preserved verbatim"
    );
    eprintln!("OBSERVED empty-visibility + owner_groups normalized to team, groups preserved");

    // ── phase 4: explicit public is untouched ──
    let resp = client
        .post(format!("{}/api/assets", pdt.base))
        .header("Authorization", auth("creator-alice"))
        .json(&serde_json::json!({
            "title": "explicit-public canary 0e583f06",
            "auth_context": {
                "visibility": "public",
                "owner_groups": [],
            },
        }))
        .send()
        .await
        .expect("create explicit public");
    assert!(resp.status().is_success());
    let created: serde_json::Value = resp.json().await.expect("create body");
    assert_eq!(
        created["auth_context"]["visibility"].as_str(),
        Some("public"),
        "explicit non-empty visibility must be honored as-is"
    );
    eprintln!("OBSERVED explicit public honored as-is");
}

// ─────────────────── the role-gate registry fallback ───────────────────

/// REAL-BINARY IT GATE — the maintenance admin gate falls back to the PDT
/// registry for claim-less exchanged tokens (CPO precondition 1, routing
/// 0ae73f46; the 6c8efe55 exchanged-token-no-claims class).
///
/// The class being closed: RFC 8693 exchange tokens (thq_admin-minted PAT,
/// aud=pdt-api) carry NO role/groups claims, and the userinfo merge for them
/// lands empty too — the token-claim admin gate alone 403s real admins
/// (paydar's finding: groups=[] despite ff766ee2 ∈ pdt-api-admins). The fix:
/// the gate falls back to RolesService (the registry-backed global_role —
/// the data-backed source the backfill syncs). Fail-closed preserved: no
/// token role AND no registry admin role = 403.
///
/// Phases (fresh-sub-per-phase is load-bearing — pep caches userinfo by sub):
///   seed      — enriched principal (userinfo Ok) stamps the registry:
///               user asset for ADMIN_SUB with sub tag + global_role=admin,
///               user asset for PLAIN_SUB with sub tag + global_role=user.
///   exchanged — userinfo Empty200 (the exchange shape); claim-less tokens:
///               ADMIN_SUB  → prune dry-run 200 (registry fallback says admin)
///               PLAIN_SUB  → 403 (registry says user — fail-closed preserved)
///               NOBODY_SUB → 403 (no registry asset — fail-closed preserved)
#[tokio::test]
async fn role_gate_falls_back_to_registry_for_claimless_admin_tokens() {
    let idp = spawn_mock_idp().await;
    let pdt = spawn_pdt(&idp).await;
    let client = wait_health(&pdt.base).await;
    eprintln!("OBSERVED pdt up at {} (mock IdP {})", pdt.base, idp.base);

    let admin_sub = "role-gate-admin-0ae73f46";
    let plain_sub = "role-gate-plain-0ae73f46";
    let nobody_sub = "role-gate-nobody-0ae73f46";
    let auth = |sub: &str| format!("Bearer {}", sign_token(&idp.base, sub));

    // ── seed phase: enriched principal (userinfo Ok, role=member) stamps
    // the registry — Create + Tag are open to all authenticated principals.
    async fn seed_principal(
        client: &reqwest::Client,
        base: &str,
        bearer: &str,
        sub: &str,
        global_role: &str,
    ) {
        let resp = client
            .post(format!("{base}/api/assets"))
            .header("Authorization", bearer)
            .json(&serde_json::json!({
                "title": format!("principal asset {sub}"),
                "content": "role-gate registry seed",
                "auth_context": {"visibility": "public", "owner_groups": []},
            }))
            .send()
            .await
            .expect("seed principal create");
        assert!(
            resp.status().is_success(),
            "seed create must succeed (got {})",
            resp.status()
        );
        let created: serde_json::Value = resp.json().await.expect("seed create body");
        let asset_id = created["_id"]
            .as_str()
            .expect("asset id (Asset serializes id as _id)")
            .to_string();
        for (category, value) in [("sub", sub), ("global_role", global_role)] {
            let resp = client
                .post(format!("{base}/api/assets/{asset_id}/tags"))
                .header("Authorization", bearer)
                .json(&serde_json::json!({"category": category, "value": value}))
                .send()
                .await
                .expect("seed add_tag");
            assert!(
                resp.status().is_success(),
                "seed add_tag {category} must succeed (got {})",
                resp.status()
            );
        }
    }

    seed_principal(&client, &pdt.base, &auth("seed-agent"), admin_sub, "admin").await;
    seed_principal(&client, &pdt.base, &auth("seed-agent-2"), plain_sub, "user").await;
    eprintln!("OBSERVED registry seeded: admin_sub + plain_sub principal assets");

    // ── exchanged phase: userinfo Empty200 — the RFC 8693 exchange shape.
    // The claim-less token (sign_token mints Kanidm-style, no identity
    // claims) enriches to EMPTY → Degraded marker → the gate must fall
    // back to the registry.
    idp.set_mode(UserinfoMode::Empty200);

    let prune_body = serde_json::json!({
        "tag_category": "type",
        "tag_value": "noise",
        "older_than_days": 14,
        "dry_run": true,
    });

    // THE FIX: claim-less ADMIN_SUB → registry fallback says admin → 200.
    let resp = client
        .post(format!("{}/api/maintenance/prune", pdt.base))
        .header("Authorization", auth(admin_sub))
        .json(&prune_body)
        .send()
        .await
        .expect("claimless admin prune dry-run");
    let status = resp.status();
    assert_eq!(
        status,
        reqwest::StatusCode::OK,
        "claim-less token with registry global_role=admin must pass the admin gate \
         via the registry fallback (the 6c8efe55 class closed end-to-end)"
    );
    let body: serde_json::Value = resp.json().await.expect("prune dry-run body");
    assert_eq!(body["dry_run"], serde_json::json!(true));
    eprintln!(
        "OBSERVED claim-less admin token → prune dry-run 200 (registry fallback fired; policy_loaded={})",
        body["policy_loaded"]
    );

    // Control 1: claim-less PLAIN_SUB → registry says user → 403 (fail-closed).
    let resp = client
        .post(format!("{}/api/maintenance/prune", pdt.base))
        .header("Authorization", auth(plain_sub))
        .json(&prune_body)
        .send()
        .await
        .expect("claimless plain prune dry-run");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "claim-less token with registry global_role=user must stay 403 (fail-closed preserved)"
    );
    eprintln!("OBSERVED claim-less user-role token → 403 (fail-closed preserved)");

    // Control 2: claim-less NOBODY_SUB → no registry asset → 403.
    let resp = client
        .post(format!("{}/api/maintenance/prune", pdt.base))
        .header("Authorization", auth(nobody_sub))
        .json(&prune_body)
        .send()
        .await
        .expect("claimless nobody prune dry-run");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::FORBIDDEN,
        "claim-less token with NO registry asset must stay 403 (least-privilege default)"
    );
    eprintln!("OBSERVED claim-less unknown-sub token → 403 (least-privilege default)");

    // Control 3: the token-claim path still works — an ENRICHED admin token
    // (userinfo Ok, role=admin) passes without touching the registry.
    idp.set_mode(UserinfoMode::OkAdmin);
    let resp = client
        .post(format!("{}/api/maintenance/prune", pdt.base))
        .header("Authorization", auth("enriched-admin"))
        .json(&prune_body)
        .send()
        .await
        .expect("enriched admin prune dry-run");
    assert_eq!(
        resp.status(),
        reqwest::StatusCode::OK,
        "enriched role=admin token must still pass (token-claim path unchanged)"
    );
    eprintln!("OBSERVED enriched role=admin token → 200 (token-claim path unchanged)");
}
