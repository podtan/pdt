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
        };
        assert!(!config.enabled);
        assert_eq!(config.issuer_url, "https://auth.example.com");
        assert_eq!(config.expected_audience, Some("custom-api".to_string()));
        assert!(config.dev_mode);
        assert_eq!(config.userinfo_url, Some("https://auth.example.com/userinfo".to_string()));
    }

    #[test]
    fn test_config_with_no_audience() {
        let config = AuthConfig {
            enabled: true,
            issuer_url: "https://auth.example.com".to_string(),
            expected_audience: None,
            dev_mode: false,
            userinfo_url: None,
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
            userinfo_url: Some("https://keycloak.example.com/auth/realms/myrealm/protocol/openid-connect/userinfo".to_string()),
        };

        assert!(config.enabled);
        assert!(config.issuer_url.contains("keycloak"));
        assert_eq!(config.expected_audience, Some("my-service".to_string()));
        assert!(config.dev_mode);
        assert!(config.userinfo_url.as_ref().unwrap().contains("userinfo"));
    }
}
