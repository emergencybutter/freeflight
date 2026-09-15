//! Per-IP rate limiting for the proxy routes (DESIGN.md §11, risk
//! [api-availability]).
//!
//! `ff-api` is an unauthenticated public proxy. The routes that matter are
//! the ones that spend *someone else's* budget on our behalf:
//! `/weather/*` fans out to aviationweather.gov, and `/notams` spends the
//! FAA NMS quota tied to our credentials — quota that can be rate-limited
//! or revoked if anonymous traffic burns it. §11 has listed this as
//! "currently unmet" since before there was a public deployment; there is
//! one now, with a web client and a mobile client pointed at it.
//!
//! A token bucket per client, rather than a fixed window: the real traffic
//! is bursty (one map pan asks for METAR, TAF, AIRMET, SIGMET, CWA, PIREPs
//! and winds at once) and a window boundary would either reject that burst
//! or have to be set so wide it stops limiting anything. A bucket absorbs
//! the burst and still caps the sustained rate.
//!
//! Hand-rolled rather than pulled from a crate because the interesting
//! part is not the algorithm, it is deciding *what the key is* behind two
//! layers of proxy — see [`client_ip`], where getting it wrong means
//! either limiting the whole internet as one client or letting anyone
//! bypass the limit with a header.

use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderName, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Sustained requests per second allowed per client, once the burst is spent.
const DEFAULT_RATE_PER_SEC: f64 = 2.0;
/// How many requests a client may make back-to-back from idle.
const DEFAULT_BURST: f64 = 30.0;
/// Buckets are dropped once idle this long — a full bucket is
/// indistinguishable from a client that has never been seen.
const IDLE_EVICTION: Duration = Duration::from_secs(300);
/// Cap on tracked clients, so rotating source addresses can't grow the map
/// without bound. Eviction runs when this is exceeded.
const MAX_TRACKED_CLIENTS: usize = 20_000;

#[derive(Debug, Clone, Copy)]
struct Bucket {
    tokens: f64,
    last_seen: Instant,
}

/// Shared limiter. Cheap to clone — the state is behind one `Arc<Mutex>`,
/// and the critical section is a hash lookup and a couple of arithmetic
/// operations, so it is not worth a sharded map at this scale.
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<Inner>,
}

struct Inner {
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
    rate_per_sec: f64,
    burst: f64,
    /// Header to read the real client address from, when a trusted proxy
    /// sets it. `None` means use the socket peer.
    client_ip_header: Option<HeaderName>,
    /// So the misconfiguration warning below is logged once, not per request.
    warned_about_proxy: AtomicBool,
}

/// How long a rejected client should wait, for the `Retry-After` header.
#[derive(Debug, PartialEq)]
pub struct RetryAfter(pub Duration);

impl RateLimiter {
    /// Reads configuration from the environment:
    ///
    /// - `FF_RATE_LIMIT_RPS` — sustained requests/sec per client
    ///   (default 2). **0 disables limiting entirely.**
    /// - `FF_RATE_LIMIT_BURST` — burst allowance (default 30).
    /// - `FF_TRUSTED_CLIENT_IP_HEADER` — e.g. `cf-connecting-ip`. Only set
    ///   this when a proxy you control *overwrites* that header on every
    ///   request; see [`client_ip`].
    pub fn from_env() -> Option<Self> {
        let rate = env_f64("FF_RATE_LIMIT_RPS").unwrap_or(DEFAULT_RATE_PER_SEC);
        if rate <= 0.0 {
            tracing::warn!("rate limiting disabled (FF_RATE_LIMIT_RPS <= 0)");
            return None;
        }
        let burst = env_f64("FF_RATE_LIMIT_BURST").unwrap_or(DEFAULT_BURST);
        let client_ip_header = std::env::var("FF_TRUSTED_CLIENT_IP_HEADER")
            .ok()
            .and_then(|name| HeaderName::try_from(name.to_lowercase()).ok());
        match &client_ip_header {
            Some(header) => tracing::info!(
                rate, burst, header = %header,
                "rate limiting proxy routes, client address from header"
            ),
            None => tracing::info!(
                rate, burst,
                "rate limiting proxy routes by socket peer address \
                 (set FF_TRUSTED_CLIENT_IP_HEADER when behind a proxy)"
            ),
        }
        Some(Self::new(rate, burst, client_ip_header))
    }

    pub fn new(rate_per_sec: f64, burst: f64, client_ip_header: Option<HeaderName>) -> Self {
        Self {
            inner: Arc::new(Inner {
                buckets: Mutex::new(HashMap::new()),
                rate_per_sec,
                burst,
                client_ip_header,
                warned_about_proxy: AtomicBool::new(false),
            }),
        }
    }

    /// Shout, once, if the address we are limiting on cannot be a real
    /// client.
    ///
    /// The failure this catches is silent and nasty: behind a reverse proxy
    /// every request arrives from the same container address, so the whole
    /// internet shares one bucket and real users start seeing 429s at a
    /// combined couple of requests per second. From outside that looks like
    /// a broken service, not a missing variable.
    ///
    /// It fires on the *resolved* key, deliberately, and not only when no
    /// header is configured. Naming a header that the proxy does not
    /// actually send is the more likely mistake of the two — the fallback
    /// to the socket peer is silent, and it lands in exactly this state —
    /// so the check that would have skipped it was the wrong check.
    fn warn_if_keying_on_a_proxy(&self, key: IpAddr) {
        if !is_private_or_loopback(key) {
            return;
        }
        if !self.inner.warned_about_proxy.swap(true, Ordering::Relaxed) {
            match &self.inner.client_ip_header {
                Some(header) => tracing::warn!(
                    key = %key, header = %header,
                    "rate limiting on a private/loopback address even though a trusted \
                     client-IP header is configured — the proxy is not sending that \
                     header, so every client is sharing one bucket. Check the header \
                     name against what the proxy actually sets."
                ),
                None => tracing::warn!(
                    key = %key,
                    "rate limiting on a private/loopback peer address — if this service \
                     is behind a reverse proxy, every client shares one bucket. Set \
                     FF_TRUSTED_CLIENT_IP_HEADER (e.g. cf-connecting-ip) to the header \
                     your proxy overwrites."
                ),
            }
        }
    }

    /// Spend one token for `key`, or report how long until one is free.
    pub fn check_at(&self, key: IpAddr, now: Instant) -> Result<(), RetryAfter> {
        let inner = &self.inner;
        let mut buckets = match inner.buckets.lock() {
            Ok(guard) => guard,
            // A poisoned lock means a previous holder panicked. Failing
            // open is the right call: this protects an upstream quota, and
            // taking the whole API down over it would be the larger outage.
            Err(poisoned) => poisoned.into_inner(),
        };

        if buckets.len() > MAX_TRACKED_CLIENTS {
            buckets.retain(|_, bucket| now.duration_since(bucket.last_seen) < IDLE_EVICTION);
        }

        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: inner.burst,
            last_seen: now,
        });
        let elapsed = now.duration_since(bucket.last_seen).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * inner.rate_per_sec).min(inner.burst);
        bucket.last_seen = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            Ok(())
        } else {
            let deficit = 1.0 - bucket.tokens;
            Err(RetryAfter(Duration::from_secs_f64(
                (deficit / inner.rate_per_sec).max(1.0),
            )))
        }
    }
}

/// The address to limit on.
///
/// In the production topology the socket peer is nginx's container address
/// on `vya2net` — identical for every request on earth, so limiting on it
/// would throttle all clients collectively as one. The real address only
/// exists in a header that a proxy set (`CF-Connecting-IP` from
/// Cloudflare, or `X-Forwarded-For` from nginx).
///
/// Trusting that header unconditionally would be worse than not limiting
/// at all: anyone could send a random value per request and never share a
/// bucket with themselves. So the header is used **only when explicitly
/// configured**, which is a statement by the operator that a proxy they
/// control overwrites it. Unset — the default, and the right default for
/// running locally — means the socket peer, which cannot be forged.
///
/// `X-Forwarded-For` accumulates a list; the first entry is the original
/// client, the rest are intermediaries.
pub fn client_ip(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    trusted_header: Option<&HeaderName>,
) -> Option<IpAddr> {
    if let Some(name) = trusted_header {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            if let Some(first) = value.split(',').next() {
                if let Ok(addr) = first.trim().parse::<IpAddr>() {
                    return Some(addr);
                }
            }
        }
    }
    peer.map(|addr| addr.ip())
}

/// Axum middleware. Applied to the proxy routes only — see `routes::router`.
pub async fn enforce(
    State(limiter): State<RateLimiter>,
    peer: Option<ConnectInfo<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let key = client_ip(
        request.headers(),
        peer.map(|ConnectInfo(addr)| addr),
        limiter.inner.client_ip_header.as_ref(),
    );
    // No address to attribute the request to (no ConnectInfo and no
    // configured header) means the limiter has nothing to key on. Letting
    // it through beats bucketing every anonymous request together.
    let Some(key) = key else {
        return next.run(request).await;
    };

    limiter.warn_if_keying_on_a_proxy(key);

    match limiter.check_at(key, Instant::now()) {
        Ok(()) => next.run(request).await,
        Err(RetryAfter(wait)) => {
            tracing::debug!(%key, "rate limited");
            (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", wait.as_secs().max(1).to_string())],
                "rate limit exceeded; this is a shared proxy for public \
                 FAA/NOAA data, please slow down\n",
            )
                .into_response()
        }
    }
}

/// Addresses that cannot be a real internet client, and therefore imply a
/// proxy sits in front of us.
fn is_private_or_loopback(addr: IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        // `fc00::/7` unique-local, plus loopback and link-local.
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

fn env_f64(name: &str) -> Option<f64> {
    std::env::var(name).ok().and_then(|v| v.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(last: u8) -> IpAddr {
        IpAddr::from([203, 0, 113, last])
    }

    fn limiter() -> RateLimiter {
        RateLimiter::new(2.0, 3.0, None)
    }

    #[test]
    fn a_burst_is_allowed_then_refused() {
        let limiter = limiter();
        let now = Instant::now();

        for i in 0..3 {
            assert!(limiter.check_at(ip(1), now).is_ok(), "request {i} of the burst");
        }
        assert!(limiter.check_at(ip(1), now).is_err(), "burst is spent");
    }

    #[test]
    fn tokens_come_back_over_time() {
        let limiter = limiter();
        let start = Instant::now();
        for _ in 0..3 {
            limiter.check_at(ip(1), start).unwrap();
        }

        // 2 tokens/sec, so half a second buys exactly one.
        let later = start + Duration::from_millis(500);
        assert!(limiter.check_at(ip(1), later).is_ok());
        assert!(limiter.check_at(ip(1), later).is_err());
    }

    #[test]
    fn refill_is_capped_at_the_burst_size() {
        let limiter = limiter();
        let start = Instant::now();
        limiter.check_at(ip(1), start).unwrap();

        // An hour of idling must not bank an hour's worth of tokens.
        let much_later = start + Duration::from_secs(3600);
        for _ in 0..3 {
            limiter.check_at(ip(1), much_later).unwrap();
        }
        assert!(limiter.check_at(ip(1), much_later).is_err());
    }

    #[test]
    fn clients_do_not_share_a_bucket() {
        let limiter = limiter();
        let now = Instant::now();
        for _ in 0..3 {
            limiter.check_at(ip(1), now).unwrap();
        }
        assert!(limiter.check_at(ip(1), now).is_err());

        // A different address is unaffected by the first one's spending.
        assert!(limiter.check_at(ip(2), now).is_ok());
    }

    #[test]
    fn retry_after_is_never_below_a_second() {
        let limiter = RateLimiter::new(100.0, 1.0, None);
        let now = Instant::now();
        limiter.check_at(ip(1), now).unwrap();

        let RetryAfter(wait) = limiter.check_at(ip(1), now).unwrap_err();
        // The real wait is 10ms, but `Retry-After` is whole seconds and 0
        // invites an immediate retry.
        assert!(wait >= Duration::from_secs(1), "got {wait:?}");
    }

    #[test]
    fn idle_clients_are_evicted_rather_than_accumulating() {
        let limiter = RateLimiter::new(2.0, 3.0, None);
        let start = Instant::now();
        for host in 0..=255u8 {
            limiter.check_at(IpAddr::from([198, 51, 100, host]), start).unwrap();
        }
        assert_eq!(limiter.inner.buckets.lock().unwrap().len(), 256);

        // Force the eviction path without synthesising 20k clients.
        {
            let mut buckets = limiter.inner.buckets.lock().unwrap();
            for host in 0..=255u16 {
                buckets.insert(
                    IpAddr::from([198, 51, (host >> 8) as u8 + 1, host as u8]),
                    Bucket { tokens: 3.0, last_seen: start },
                );
            }
            while buckets.len() <= MAX_TRACKED_CLIENTS {
                let n = buckets.len() as u32;
                buckets.insert(
                    IpAddr::from(std::net::Ipv4Addr::from(n.wrapping_add(1 << 24))),
                    Bucket { tokens: 3.0, last_seen: start },
                );
            }
        }
        let long_after = start + IDLE_EVICTION + Duration::from_secs(1);
        limiter.check_at(ip(9), long_after).unwrap();

        let remaining = limiter.inner.buckets.lock().unwrap().len();
        assert!(remaining < 100, "idle buckets should be gone, {remaining} left");
    }

    #[test]
    fn proxy_addresses_are_recognised() {
        // Docker/nginx and loopback: a proxy is in front.
        assert!(is_private_or_loopback("172.18.0.4".parse().unwrap()));
        assert!(is_private_or_loopback("10.0.0.1".parse().unwrap()));
        assert!(is_private_or_loopback("192.168.1.1".parse().unwrap()));
        assert!(is_private_or_loopback("127.0.0.1".parse().unwrap()));
        assert!(is_private_or_loopback("::1".parse().unwrap()));
        assert!(is_private_or_loopback("fd00::1".parse().unwrap()));
        // Real clients.
        assert!(!is_private_or_loopback("203.0.113.1".parse().unwrap()));
        assert!(!is_private_or_loopback("2001:db8::1".parse().unwrap()));
    }

    // ---- key extraction ---------------------------------------------------

    fn headers_with(name: &str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::try_from(name.to_string()).unwrap(),
            value.parse().unwrap(),
        );
        headers
    }

    #[test]
    fn the_socket_peer_is_used_when_no_header_is_trusted() {
        let headers = headers_with("x-forwarded-for", "198.51.100.7");
        let peer: SocketAddr = "203.0.113.1:1234".parse().unwrap();

        // Not configured, so the header is ignored even though it is here.
        assert_eq!(client_ip(&headers, Some(peer), None), Some(ip(1)));
    }

    #[test]
    fn a_configured_header_wins_over_the_peer() {
        let headers = headers_with("cf-connecting-ip", "198.51.100.7");
        let peer: SocketAddr = "203.0.113.1:1234".parse().unwrap();
        let trusted = HeaderName::from_static("cf-connecting-ip");

        assert_eq!(
            client_ip(&headers, Some(peer), Some(&trusted)),
            Some(IpAddr::from([198, 51, 100, 7]))
        );
    }

    #[test]
    fn a_forwarded_chain_is_attributed_to_the_original_client() {
        let headers = headers_with("x-forwarded-for", "198.51.100.7, 10.0.0.1, 10.0.0.2");
        let trusted = HeaderName::from_static("x-forwarded-for");

        assert_eq!(
            client_ip(&headers, None, Some(&trusted)),
            Some(IpAddr::from([198, 51, 100, 7]))
        );
    }

    #[test]
    fn a_garbage_header_falls_back_to_the_peer() {
        let headers = headers_with("cf-connecting-ip", "not-an-address");
        let peer: SocketAddr = "203.0.113.1:1234".parse().unwrap();
        let trusted = HeaderName::from_static("cf-connecting-ip");

        assert_eq!(client_ip(&headers, Some(peer), Some(&trusted)), Some(ip(1)));
    }
}
