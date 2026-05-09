// Tiny per-IP fixed-window rate limiter.
//
// SECURITY: Sits in front of donation checkout, sound upload, and Stripe
// Connect onboarding so a single hostile IP cannot spam Stripe API calls,
// flood the disk with upload attempts, or churn through onboarding links.
//
// We deliberately avoid pulling in `tower_governor` or `dashmap` —
// the requirements (a handful of routes, fixed window, per-IP) are easy
// enough to satisfy with `Arc<Mutex<HashMap<...>>>`. The map is bounded
// in practice by the limiter's pruning step, which evicts entries whose
// window has fully expired on every check.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use tokio::sync::Mutex;

/// One bucket = one IP within one window.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    window_start: Instant,
    count: u32,
}

/// Per-route limiter. Cheap to clone (`Arc` inside) so each route can
/// own a handle.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
    max_per_window: u32,
    window: Duration,
    /// Human-readable label used in 429 bodies and tracing logs.
    label: &'static str,
}

impl RateLimiter {
    /// `max` requests per `window_secs` seconds.
    pub fn new(label: &'static str, max: u32, window_secs: u64) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            max_per_window: max,
            window: Duration::from_secs(window_secs),
            label,
        }
    }

    /// Returns `Ok(())` if the IP is under the limit, or
    /// `Err(retry_after_secs)` if it has been throttled.
    async fn check(&self, ip: IpAddr) -> Result<(), u64> {
        let now = Instant::now();
        let mut guard = self.inner.lock().await;

        // Lazy prune: drop fully-expired buckets so the map can't grow
        // unbounded under heavy IP churn.
        guard.retain(|_, b| now.duration_since(b.window_start) < self.window);

        let bucket = guard.entry(ip).or_insert(Bucket {
            window_start: now,
            count: 0,
        });

        if now.duration_since(bucket.window_start) >= self.window {
            bucket.window_start = now;
            bucket.count = 0;
        }

        if bucket.count >= self.max_per_window {
            let retry = self
                .window
                .saturating_sub(now.duration_since(bucket.window_start))
                .as_secs()
                .max(1);
            return Err(retry);
        }
        bucket.count += 1;
        Ok(())
    }
}

/// Axum middleware: pulls the connect-info IP and consults the limiter.
pub async fn rate_limit_layer(
    limiter: RateLimiter,
    request: Request<Body>,
    next: Next,
) -> Response {
    // SECURITY: We pull the IP from `ConnectInfo`, not from
    // `X-Forwarded-For`. Trusting forwarded headers without a vetted
    // proxy chain would let any client spoof a fresh IP per request and
    // bypass the limit. Operators terminating TLS at a proxy must add
    // their own header-based limiter at that layer.
    let ip = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.0.ip())
        .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)));

    match limiter.check(ip).await {
        Ok(()) => next.run(request).await,
        Err(retry_after) => {
            tracing::warn!(
                "rate limit hit on {} from {ip}: retry after {retry_after}s",
                limiter.label
            );
            let body = axum::Json(json!({
                "error": format!("rate limit exceeded for {}", limiter.label),
            }));
            let mut resp = (StatusCode::TOO_MANY_REQUESTS, body).into_response();
            resp.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                retry_after.to_string().parse().unwrap_or_else(|_| {
                    axum::http::HeaderValue::from_static("60")
                }),
            );
            resp
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn allows_up_to_limit_then_throttles() {
        let limiter = RateLimiter::new("test", 3, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        for _ in 0..3 {
            assert!(limiter.check(ip).await.is_ok());
        }
        assert!(limiter.check(ip).await.is_err());
    }

    #[tokio::test]
    async fn separate_ips_have_separate_buckets() {
        let limiter = RateLimiter::new("test", 1, 60);
        let a = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let b = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        assert!(limiter.check(a).await.is_ok());
        assert!(limiter.check(b).await.is_ok());
        assert!(limiter.check(a).await.is_err());
    }

    #[tokio::test]
    async fn returns_retry_after_seconds_in_window() {
        let limiter = RateLimiter::new("test", 1, 60);
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        limiter.check(ip).await.unwrap();
        let err = limiter.check(ip).await.unwrap_err();
        assert!((1..=60).contains(&err));
    }
}
