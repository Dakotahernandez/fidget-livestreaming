use std::env;


#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub rtmp_url: String,
    pub quic_port: u16,
    pub cloudfront_url: String,
    pub stripe_secret_key: String,
    pub stripe_webhook_secret: String,
    /// Reserved for a future Standard-account OAuth flow; the current
    /// Express + Account Links onboarding doesn't use it.
    #[allow(dead_code)]
    pub stripe_client_id: String,
    pub upload_dir: String,
    pub upload_max_bytes: u64,
    pub sounds_per_streamer_max: i64,
    pub frontend_url: String,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Config")
            .field("database_url", &"<redacted>")
            .field("jwt_secret", &"<redacted>")
            .field("rtmp_url", &self.rtmp_url)
            .field("quic_port", &self.quic_port)
            .field("cloudfront_url", &self.cloudfront_url)
            .field("stripe_secret_key", &"<redacted>")
            .field("stripe_webhook_secret", &"<redacted>")
            .field("stripe_client_id", &"<redacted>")
            .field("upload_dir", &self.upload_dir)
            .field("upload_max_bytes", &self.upload_max_bytes)
            .field("sounds_per_streamer_max", &self.sounds_per_streamer_max)
            .field("frontend_url", &self.frontend_url)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let jwt_secret = env::var("JWT_SECRET")
            .map_err(|_| ConfigError::Missing("JWT_SECRET"))?;
        let rtmp_url = env::var("RTMP_URL")
            .unwrap_or_else(|_| "rtmp://fidgetlive.com/live".to_string());
        let quic_port: u16 = env::var("QUIC_PORT")
            .unwrap_or_else(|_| "4443".to_string())
            .parse()
            .unwrap_or(4443);
        let cloudfront_url = env::var("CLOUDFRONT_DOMAIN")
            .unwrap_or_else(|_| "https://d1wp4v4rt1bxg5.cloudfront.net".to_string());

        // Stripe + uploads. Required keys error explicitly so misconfigured
        // deployments fail fast rather than 500ing only when a donor tries
        // to checkout.
        let stripe_secret_key = env::var("STRIPE_SECRET_KEY")
            .map_err(|_| ConfigError::Missing("STRIPE_SECRET_KEY"))?;
        let stripe_webhook_secret = env::var("STRIPE_WEBHOOK_SECRET")
            .map_err(|_| ConfigError::Missing("STRIPE_WEBHOOK_SECRET"))?;
        // STRIPE_CLIENT_ID is required by the spec for forward-compat with
        // a future Standard-account OAuth flow; the current Express +
        // Account Links onboarding doesn't actually use it.
        let stripe_client_id = env::var("STRIPE_CLIENT_ID").unwrap_or_default();
        let frontend_url = env::var("FRONTEND_URL")
            .map_err(|_| ConfigError::Missing("FRONTEND_URL"))?;

        let upload_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "/app/uploads".to_string());
        let upload_max_bytes: u64 = env::var("UPLOAD_MAX_BYTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2_097_152);
        let sounds_per_streamer_max: i64 = env::var("SOUNDS_PER_STREAMER_MAX")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        Ok(Self {
            database_url,
            jwt_secret,
            rtmp_url,
            quic_port,
            cloudfront_url,
            stripe_secret_key,
            stripe_webhook_secret,
            stripe_client_id,
            upload_dir,
            upload_max_bytes,
            sounds_per_streamer_max,
            frontend_url,
        })
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Missing(&'static str),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(var) => write!(f, "missing required environment variable: {var}"),
        }
    }
}

impl std::error::Error for ConfigError {}
