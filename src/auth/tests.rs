//! Unit tests for auth module

#[cfg(test)]
mod config_tests {
    use crate::auth::config::AuthConfig;

    #[test]
    fn test_default_config() {
        let config = AuthConfig::default();
        assert!(config.enabled);
        assert_eq!(config.issuer_url, "http://localhost:8080");
        assert_eq!(config.expected_audience, None);
        assert!(!config.dev_mode);
        assert_eq!(config.userinfo_url, None);
    }

    #[test]
    fn test_custom_config() {
        let config = AuthConfig {
            enabled: false,
            issuer_url: "https://auth.example.com".to_string(),
            expected_audience: Some("custom-api".to_string()),
            dev_mode: true,
            userinfo_url: Some("https://auth.example.com/userinfo".to_string()),
            trusted_issuers: None,
        };
        assert!(!config.enabled);
        assert_eq!(config.issuer_url, "https://auth.example.com");
        assert_eq!(config.expected_audience, Some("custom-api".to_string()));
        assert!(config.dev_mode);
        assert_eq!(
            config.userinfo_url,
            Some("https://auth.example.com/userinfo".to_string())
        );
    }

    #[test]
    fn test_config_with_no_audience() {
        let config = AuthConfig {
            enabled: true,
            issuer_url: "https://auth.example.com".to_string(),
            expected_audience: None,
            dev_mode: false,
            userinfo_url: None,
            trusted_issuers: None,
        };
        assert!(config.expected_audience.is_none());
        assert!(config.userinfo_url.is_none());
    }
}

#[cfg(test)]
mod error_tests {
    use crate::auth::error::AuthError;

    #[test]
    fn test_missing_token_error_display() {
        let error = AuthError::MissingToken;
        assert_eq!(error.to_string(), "Missing authentication token");
    }

    #[test]
    fn test_invalid_token_error_display() {
        let error = AuthError::InvalidToken("malformed".to_string());
        let display = error.to_string();
        assert!(display.contains("Invalid token"));
    }

    #[test]
    fn test_invalid_claims_error_display() {
        let error = AuthError::InvalidClaims("missing sub".to_string());
        let display = error.to_string();
        assert!(display.contains("Invalid claims"));
    }

    #[test]
    fn test_error_is_std_error() {
        use std::error::Error;
        let error: Box<dyn Error> = Box::new(AuthError::MissingToken);
        assert!(!error.to_string().is_empty());
    }
}

#[cfg(test)]
mod middleware_tests {
    use axum::http::{HeaderMap, HeaderValue};

    #[test]
    fn test_bearer_token_extraction_from_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Bearer valid_jwt_token_here"),
        );

        // Get the header value to verify it's set
        let auth_header = headers.get("Authorization");
        assert!(auth_header.is_some());
        assert_eq!(
            auth_header.unwrap().to_str().unwrap(),
            "Bearer valid_jwt_token_here"
        );
    }

    #[test]
    fn test_missing_authorization_header() {
        let headers = HeaderMap::new();
        assert!(headers.get("Authorization").is_none());
    }

    #[test]
    fn test_invalid_authorization_header_format() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("InvalidFormat token"),
        );

        let auth_header = headers.get("Authorization");
        assert!(auth_header.is_some());
        let auth_str = auth_header.unwrap().to_str().unwrap();
        assert!(!auth_str.starts_with("Bearer "));
    }

    #[test]
    fn test_basic_auth_header_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "Authorization",
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );

        let auth_header = headers.get("Authorization");
        assert!(auth_header.is_some());
        let auth_str = auth_header.unwrap().to_str().unwrap();
        assert!(!auth_str.starts_with("Bearer "));
    }
}

#[cfg(test)]
mod authenticated_user_tests {
    use crate::auth::AuthenticatedUser;

    #[test]
    fn test_authenticated_user_creation() {
        let user = AuthenticatedUser {
            user_id: "user123".to_string(),
            email: Some("user@example.com".to_string()),
            username: Some("john_doe".to_string()),
            claims_extra: None,
        };

        assert_eq!(user.user_id, "user123");
        assert_eq!(user.email, Some("user@example.com".to_string()));
        assert_eq!(user.username, Some("john_doe".to_string()));
    }

    #[test]
    fn test_authenticated_user_without_optional_fields() {
        let user = AuthenticatedUser {
            user_id: "user456".to_string(),
            email: None,
            username: None,
            claims_extra: None,
        };

        assert_eq!(user.user_id, "user456");
        assert!(user.email.is_none());
        assert!(user.username.is_none());
    }

    #[test]
    fn test_authenticated_user_partial_optional_fields() {
        let user = AuthenticatedUser {
            user_id: "user789".to_string(),
            email: Some("user@test.com".to_string()),
            username: None,
            claims_extra: None,
        };

        assert_eq!(user.user_id, "user789");
        assert_eq!(user.email, Some("user@test.com".to_string()));
        assert!(user.username.is_none());
    }

    #[test]
    fn test_authenticated_user_all_fields() {
        let user = AuthenticatedUser {
            user_id: "admin@example.com".to_string(),
            email: Some("admin@example.com".to_string()),
            username: Some("admin".to_string()),
            claims_extra: None,
        };

        assert_eq!(user.user_id, "admin@example.com");
        assert!(user.email.is_some());
        assert!(user.username.is_some());
    }
}

#[cfg(test)]
mod auth_config_integration_tests {
    use crate::auth::config::AuthConfig;

    #[test]
    fn test_config_clone() {
        let config1 = AuthConfig {
            enabled: true,
            issuer_url: "https://auth.example.com".to_string(),
            expected_audience: Some("test-api".to_string()),
            dev_mode: false,
            userinfo_url: None,
            trusted_issuers: None,
        };

        let config2 = config1.clone();
        assert_eq!(config1.enabled, config2.enabled);
        assert_eq!(config1.issuer_url, config2.issuer_url);
        assert_eq!(config1.expected_audience, config2.expected_audience);
        assert_eq!(config1.dev_mode, config2.dev_mode);
        assert_eq!(config1.userinfo_url, config2.userinfo_url);
    }

    #[test]
    fn test_config_debug_representation() {
        let config = AuthConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("AuthConfig"));
        assert!(debug_str.contains("enabled"));
    }

    #[test]
    fn test_config_with_all_fields() {
        let config = AuthConfig {
            enabled: true,
            issuer_url: "https://keycloak.example.com/auth/realms/myrealm".to_string(),
            expected_audience: Some("my-service".to_string()),
            dev_mode: true,
            userinfo_url: Some(
                "https://keycloak.example.com/auth/realms/myrealm/protocol/openid-connect/userinfo"
                    .to_string(),
            ),
            trusted_issuers: None,
        };

        assert!(config.enabled);
        assert!(config.issuer_url.contains("keycloak"));
        assert_eq!(config.expected_audience, Some("my-service".to_string()));
        assert!(config.dev_mode);
        assert!(config.userinfo_url.as_ref().unwrap().contains("userinfo"));
    }
}

// ============================================================================
// ENRICHMENT CLASSIFICATION (issue 365be03f)
// ============================================================================

#[cfg(test)]
mod enrichment_classification {
    use crate::auth::middleware::{classify_enrichment, AuthEnrichment};
    use pep::oidc::types::JwtClaims;

    fn claims_with(extra: &[(&str, serde_json::Value)]) -> JwtClaims {
        JwtClaims {
            sub: "u-1".to_string(),
            iss: "test-issuer".to_string(),
            aud: None,
            exp: 9_999_999_999,
            iat: Some(0),
            email: None,
            name: None,
            preferred_username: None,
            extra: extra
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        }
    }

    #[test]
    fn groups_present_is_enriched() {
        let claims = claims_with(&[("groups", serde_json::json!(["plt-agents"]))]);
        assert_eq!(classify_enrichment(&claims), AuthEnrichment::Enriched);
    }

    #[test]
    fn role_only_is_enriched() {
        // Service accounts with a mapped role carry real authorization
        // material — Cedar's evaluation with it is honest (no lie possible).
        let claims = claims_with(&[("role", serde_json::json!("admin"))]);
        assert_eq!(classify_enrichment(&claims), AuthEnrichment::Enriched);
    }

    #[test]
    fn zero_attributes_is_degraded() {
        // The exact incident shape: Kanidm AT carries no identity claims and
        // the userinfo merge failed (pep swallows the HTTP error as Ok) —
        // a zero-attribute principal must NOT read as enriched.
        let claims = claims_with(&[]);
        assert_eq!(classify_enrichment(&claims), AuthEnrichment::Degraded);
    }

    #[test]
    fn unrelated_extra_fields_do_not_count_as_material() {
        // email/jti etc. are not authorization material.
        let claims = claims_with(&[("email", serde_json::json!("a@b.c"))]);
        assert_eq!(classify_enrichment(&claims), AuthEnrichment::Degraded);
    }
}

#[cfg(test)]
mod trusted_issuers_tests {
    //! The 0.3.23 trusted-issuers knob (issue 895cbcea — the CPO IdP
    //! cut-over re-arm, gate-1 duty per issuer-law v2).

    use crate::auth::build_auth_client;
    use crate::auth::config::AuthConfig;

    #[test]
    fn test_parse_csv_two_issuers() {
        let parsed = AuthConfig::parse_trusted_issuers(Some(
            "https://idm.podai.ir,https://idm.example.com".to_string(),
        ));
        assert_eq!(
            parsed,
            Some(vec![
                "https://idm.podai.ir".to_string(),
                "https://idm.example.com".to_string()
            ])
        );
    }

    #[test]
    fn test_parse_trims_and_normalizes_trailing_slashes() {
        let parsed = AuthConfig::parse_trusted_issuers(Some(
            "  https://idm.podai.ir/  , https://idm.example.com ".to_string(),
        ));
        assert_eq!(
            parsed,
            Some(vec![
                "https://idm.podai.ir".to_string(),
                "https://idm.example.com".to_string()
            ])
        );
    }

    #[test]
    fn test_parse_drops_empties() {
        let parsed =
            AuthConfig::parse_trusted_issuers(Some("https://idm.podai.ir,, ,".to_string()));
        assert_eq!(parsed, Some(vec!["https://idm.podai.ir".to_string()]));
    }

    #[test]
    fn test_parse_unset_is_none() {
        assert_eq!(AuthConfig::parse_trusted_issuers(None), None);
    }

    #[test]
    fn test_parse_empty_string_is_none() {
        assert_eq!(AuthConfig::parse_trusted_issuers(Some(String::new())), None);
    }

    #[test]
    fn test_parse_all_empty_csv_is_none() {
        assert_eq!(
            AuthConfig::parse_trusted_issuers(Some(" , ,".to_string())),
            None
        );
    }

    #[test]
    fn test_default_config_has_no_allowlist() {
        let config = AuthConfig::default();
        assert_eq!(config.trusted_issuers, None);
    }

    /// THE BYTE-IDENTICAL LAW: unset/empty allowlist → the client is
    /// constructed WITHOUT the knob — trusted_issuers stays EMPTY (strict
    /// single-issuer, fail-closed preserved).
    #[tokio::test]
    async fn test_build_auth_client_unset_is_byte_identical() {
        let config = AuthConfig {
            trusted_issuers: None,
            ..AuthConfig::default()
        };
        let client = build_auth_client(&config);
        assert!(
            client.trusted_issuers.is_empty(),
            "unset allowlist must construct WITHOUT the knob — got {:?}",
            client.trusted_issuers
        );

        let config_empty = AuthConfig {
            trusted_issuers: Some(Vec::new()),
            ..AuthConfig::default()
        };
        let client_empty = build_auth_client(&config_empty);
        assert!(
            client_empty.trusted_issuers.is_empty(),
            "empty allowlist must construct WITHOUT the knob"
        );
    }

    /// The armed path: a non-empty allowlist lands on the client,
    /// normalized (trimmed, trailing slashes dropped).
    #[tokio::test]
    async fn test_build_auth_client_armed_lands_the_allowlist() {
        let config = AuthConfig {
            trusted_issuers: Some(vec![
                "https://idm.podai.ir/".to_string(),
                "  https://idm.example.com  ".to_string(),
            ]),
            ..AuthConfig::default()
        };
        let client = build_auth_client(&config);
        assert_eq!(
            client.trusted_issuers,
            vec![
                "https://idm.podai.ir".to_string(),
                "https://idm.example.com".to_string()
            ]
        );
    }
}

#[cfg(test)]
mod enrichment_target_tests {
    //! Enrichment follows the VALIDATED issuer (issue 895cbcea cut-over leg
    //! — paydar's finding 2026-11-01): after the trusted-issuer fallback
    //! validates a token minted by a non-configured issuer, enrichment must
    //! hit THAT issuer's userinfo, and the `userinfo_url` override (a pin on
    //! the CONFIGURED issuer's endpoint) must not hijack the request.

    use crate::auth::config::AuthConfig;
    use crate::auth::middleware::enrichment_target;
    use pep::oidc::types::JwtClaims;

    fn claims_with_iss(iss: &str) -> JwtClaims {
        JwtClaims {
            sub: "u-1".to_string(),
            iss: iss.to_string(),
            aud: None,
            exp: 9_999_999_999,
            iat: Some(0),
            email: None,
            name: None,
            preferred_username: None,
            extra: Default::default(),
        }
    }

    fn config_with_override() -> AuthConfig {
        AuthConfig {
            issuer_url: "https://idm.tanbal.example/oauth2/openid/pdt-api".to_string(),
            userinfo_url: Some("https://idm.tanbal.example/userinfo".to_string()),
            ..AuthConfig::default()
        }
    }

    #[test]
    fn primary_token_keeps_configured_issuer_and_override() {
        // Byte-identical primary path: validated iss == configured issuer
        // (validation enforces it) → the configured issuer + the override
        // as-is.
        let config = config_with_override();
        let claims = claims_with_iss("https://idm.tanbal.example/oauth2/openid/pdt-api");
        let (issuer, override_url) = enrichment_target(&config, &claims);
        assert_eq!(issuer, config.issuer_url);
        assert_eq!(
            override_url.as_deref(),
            Some("https://idm.tanbal.example/userinfo")
        );
    }

    #[test]
    fn fallback_token_follows_the_validated_issuer() {
        // The fallback fired: claims.iss = the token's own (allowlisted)
        // issuer ≠ configured issuer → enrichment targets claims.iss and the
        // override (pinned to the CONFIGURED issuer's endpoint) is dropped.
        let config = config_with_override();
        let claims = claims_with_iss("https://idm.podai.example/oauth2/openid/pdt-api");
        let (issuer, override_url) = enrichment_target(&config, &claims);
        assert_eq!(issuer, "https://idm.podai.example/oauth2/openid/pdt-api");
        assert_eq!(override_url, None, "the override pins the configured issuer's endpoint — it must not hijack a fallback-validated request");
    }

    #[test]
    fn trailing_slash_variants_are_the_primary_path() {
        // Issuer equality normalizes trailing slashes (mirrors pep's
        // issuer_matches): a trailing-slash difference between the validated
        // iss and the configured issuer is NOT a fallback.
        let config = config_with_override();
        let claims = claims_with_iss("https://idm.tanbal.example/oauth2/openid/pdt-api/");
        let (issuer, override_url) = enrichment_target(&config, &claims);
        assert_eq!(issuer, config.issuer_url);
        assert!(override_url.is_some());
    }

    #[test]
    fn empty_iss_guards_to_the_configured_issuer() {
        // Never post-validation (validation enforces iss), guarded anyway:
        // an empty iss must not route enrichment to "".
        let config = config_with_override();
        let claims = claims_with_iss("");
        let (issuer, override_url) = enrichment_target(&config, &claims);
        assert_eq!(issuer, config.issuer_url);
        assert!(override_url.is_some());
    }
}
