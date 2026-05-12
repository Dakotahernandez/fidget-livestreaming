use std::env;

/// Runtime configuration pulled from environment variables.
#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,    //Postgres DSN.
    pub jwt_secret: String,      //HS256 signing key for JWTs.
    pub rtmp_url: String,        //RTMP base URL ffmpeg pushes to (SRS).
    pub quic_port: u16,          //UDP port the WebTransport server binds.
    pub cloudfront_url: String,  //CDN origin for HLS playback.
    pub admin_username: String,
    pub admin_password: String,
}

impl Config {
    //Constructor that reads from env. Falls back to dev defaults where it can.
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
        // Optional external HLS base URL (CloudFront in production).
        // When unset, callers fall back to a relative `/hls/<key>/index.m3u8`
        // path which the frontend nginx proxies to the in-cluster
        // media-server. This keeps local docker-compose dev working
        // without any deployment-specific env overrides.
        let cloudfront_url = env::var("CLOUDFRONT_DOMAIN").unwrap_or_default();
        let admin_username = env::var("ADMIN_USERNAME")
            .map_err(|_| ConfigError::Missing("ADMIN_USERNAME"))?;
        let admin_password = env::var("ADMIN_PASSWORD")
            .map_err(|_| ConfigError::Missing("ADMIN_PASSWORD"))?;

        Ok(Self {
            database_url,
            jwt_secret,
            rtmp_url,
            quic_port,
            cloudfront_url,
            admin_username,
            admin_password,
        })
    }
}

//Error for missing required env vars.
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
