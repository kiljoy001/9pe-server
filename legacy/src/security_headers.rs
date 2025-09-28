//! Security Headers Middleware
//!
//! Implements comprehensive security headers for HTTP responses
//! to protect against common web vulnerabilities

use axum::{
    body::Body,
    http::{header, HeaderValue, Request, Response, StatusCode},
    middleware::Next,
};
use std::time::Duration;
use tracing::debug;

/// Security header configuration
#[derive(Clone)]
pub struct SecurityHeadersConfig {
    /// Enable Strict-Transport-Security
    pub enable_hsts: bool,
    /// HSTS max age in seconds
    pub hsts_max_age: u64,
    /// Include subdomains in HSTS
    pub hsts_include_subdomains: bool,
    /// Enable HSTS preload
    pub hsts_preload: bool,

    /// Content-Security-Policy directives
    pub csp_default_src: String,
    pub csp_script_src: String,
    pub csp_style_src: String,
    pub csp_img_src: String,
    pub csp_connect_src: String,
    pub csp_font_src: String,
    pub csp_object_src: String,
    pub csp_media_src: String,
    pub csp_frame_src: String,
    pub csp_sandbox: Option<String>,
    pub csp_report_uri: Option<String>,
    pub csp_upgrade_insecure_requests: bool,

    /// X-Frame-Options value
    pub x_frame_options: String,

    /// X-Content-Type-Options
    pub x_content_type_options: String,

    /// Referrer-Policy
    pub referrer_policy: String,

    /// Permissions-Policy
    pub permissions_policy: String,

    /// Cross-Origin policies
    pub cross_origin_embedder_policy: String,
    pub cross_origin_opener_policy: String,
    pub cross_origin_resource_policy: String,
}

impl Default for SecurityHeadersConfig {
    fn default() -> Self {
        Self {
            // HSTS settings
            enable_hsts: true,
            hsts_max_age: 31536000, // 1 year
            hsts_include_subdomains: true,
            hsts_preload: true,

            // Conservative CSP policy
            csp_default_src: "'self'".to_string(),
            csp_script_src: "'self' 'unsafe-inline'".to_string(),
            csp_style_src: "'self' 'unsafe-inline'".to_string(),
            csp_img_src: "'self' data: https:".to_string(),
            csp_connect_src: "'self'".to_string(),
            csp_font_src: "'self' data:".to_string(),
            csp_object_src: "'none'".to_string(),
            csp_media_src: "'self'".to_string(),
            csp_frame_src: "'none'".to_string(),
            csp_sandbox: None,
            csp_report_uri: None,
            csp_upgrade_insecure_requests: true,

            // Prevent clickjacking
            x_frame_options: "DENY".to_string(),

            // Prevent MIME type sniffing
            x_content_type_options: "nosniff".to_string(),

            // Control referrer information
            referrer_policy: "strict-origin-when-cross-origin".to_string(),

            // Permissions policy (Feature policy successor)
            permissions_policy: "geolocation=(), camera=(), microphone=()".to_string(),

            // Cross-origin policies
            cross_origin_embedder_policy: "require-corp".to_string(),
            cross_origin_opener_policy: "same-origin".to_string(),
            cross_origin_resource_policy: "same-origin".to_string(),
        }
    }
}

impl SecurityHeadersConfig {
    /// Create a strict configuration for maximum security
    pub fn strict() -> Self {
        Self {
            // Very strict CSP
            csp_default_src: "'none'".to_string(),
            csp_script_src: "'self'".to_string(),
            csp_style_src: "'self'".to_string(),
            csp_img_src: "'self'".to_string(),
            csp_connect_src: "'self'".to_string(),
            csp_font_src: "'self'".to_string(),
            csp_object_src: "'none'".to_string(),
            csp_media_src: "'none'".to_string(),
            csp_frame_src: "'none'".to_string(),
            csp_sandbox: Some("allow-scripts allow-same-origin".to_string()),
            csp_upgrade_insecure_requests: true,

            // Strictest referrer policy
            referrer_policy: "no-referrer".to_string(),

            // Deny all permissions
            permissions_policy: "geolocation=(), camera=(), microphone=(), payment=(), usb=(), magnetometer=(), gyroscope=(), accelerometer=()".to_string(),

            ..Default::default()
        }
    }

    /// Build the Content-Security-Policy header value
    fn build_csp(&self) -> String {
        let mut csp = Vec::new();

        csp.push(format!("default-src {}", self.csp_default_src));
        csp.push(format!("script-src {}", self.csp_script_src));
        csp.push(format!("style-src {}", self.csp_style_src));
        csp.push(format!("img-src {}", self.csp_img_src));
        csp.push(format!("connect-src {}", self.csp_connect_src));
        csp.push(format!("font-src {}", self.csp_font_src));
        csp.push(format!("object-src {}", self.csp_object_src));
        csp.push(format!("media-src {}", self.csp_media_src));
        csp.push(format!("frame-src {}", self.csp_frame_src));

        if let Some(ref sandbox) = self.csp_sandbox {
            csp.push(format!("sandbox {}", sandbox));
        }

        if self.csp_upgrade_insecure_requests {
            csp.push("upgrade-insecure-requests".to_string());
        }

        if let Some(ref report_uri) = self.csp_report_uri {
            csp.push(format!("report-uri {}", report_uri));
        }

        csp.join("; ")
    }

    /// Build the Strict-Transport-Security header value
    fn build_hsts(&self) -> String {
        if !self.enable_hsts {
            return String::new();
        }

        let mut hsts = format!("max-age={}", self.hsts_max_age);

        if self.hsts_include_subdomains {
            hsts.push_str("; includeSubDomains");
        }

        if self.hsts_preload {
            hsts.push_str("; preload");
        }

        hsts
    }
}

/// Middleware function to add security headers to responses
pub async fn security_headers_middleware(
    config: SecurityHeadersConfig,
    request: Request<Body>,
    next: Next,
) -> Result<Response<Body>, StatusCode> {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Add HSTS header
    if config.enable_hsts {
        let hsts_value = config.build_hsts();
        if !hsts_value.is_empty() {
            headers.insert(
                header::STRICT_TRANSPORT_SECURITY,
                HeaderValue::from_str(&hsts_value).unwrap_or_else(|_| HeaderValue::from_static("")),
            );
        }
    }

    // Add Content-Security-Policy
    let csp_value = config.build_csp();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_str(&csp_value).unwrap_or_else(|_| HeaderValue::from_static("")),
    );

    // Add X-Frame-Options
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_str(&config.x_frame_options).unwrap_or_else(|_| HeaderValue::from_static("DENY")),
    );

    // Add X-Content-Type-Options
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_str(&config.x_content_type_options).unwrap_or_else(|_| HeaderValue::from_static("nosniff")),
    );

    // Add Referrer-Policy
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_str(&config.referrer_policy).unwrap_or_else(|_| HeaderValue::from_static("strict-origin")),
    );

    // Add Permissions-Policy
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_str(&config.permissions_policy).unwrap_or_else(|_| HeaderValue::from_static("")),
    );

    // Add Cross-Origin-Embedder-Policy
    headers.insert(
        "Cross-Origin-Embedder-Policy",
        HeaderValue::from_str(&config.cross_origin_embedder_policy).unwrap_or_else(|_| HeaderValue::from_static("require-corp")),
    );

    // Add Cross-Origin-Opener-Policy
    headers.insert(
        "Cross-Origin-Opener-Policy",
        HeaderValue::from_str(&config.cross_origin_opener_policy).unwrap_or_else(|_| HeaderValue::from_static("same-origin")),
    );

    // Add Cross-Origin-Resource-Policy
    headers.insert(
        "Cross-Origin-Resource-Policy",
        HeaderValue::from_str(&config.cross_origin_resource_policy).unwrap_or_else(|_| HeaderValue::from_static("same-origin")),
    );

    // Remove potentially dangerous headers
    headers.remove("X-Powered-By");
    headers.remove("Server");

    // Add cache control for sensitive content
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
        );
    }

    debug!("Security headers applied to response");
    Ok(response)
}

/// Create a layer that can be added to an Axum router
pub fn security_headers_layer(config: SecurityHeadersConfig) -> impl Fn(Request<Body>, Next) -> futures::future::BoxFuture<'static, Result<Response<Body>, StatusCode>> + Clone + Send + 'static {
    move |req, next| {
        let config = config.clone();
        Box::pin(security_headers_middleware(config, req, next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
        middleware::Next,
        response::Response,
    };

    #[test]
    fn test_csp_building() {
        let config = SecurityHeadersConfig::default();
        let csp = config.build_csp();

        assert!(csp.contains("default-src 'self'"));
        assert!(csp.contains("script-src 'self' 'unsafe-inline'"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("upgrade-insecure-requests"));
    }

    #[test]
    fn test_strict_csp() {
        let config = SecurityHeadersConfig::strict();
        let csp = config.build_csp();

        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(!csp.contains("'unsafe-inline'"));
    }

    #[test]
    fn test_hsts_building() {
        let config = SecurityHeadersConfig::default();
        let hsts = config.build_hsts();

        assert!(hsts.contains("max-age=31536000"));
        assert!(hsts.contains("includeSubDomains"));
        assert!(hsts.contains("preload"));
    }

    #[test]
    fn test_hsts_disabled() {
        let mut config = SecurityHeadersConfig::default();
        config.enable_hsts = false;
        let hsts = config.build_hsts();

        assert!(hsts.is_empty());
    }
}