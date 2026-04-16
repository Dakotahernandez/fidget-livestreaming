use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
    pub rtmp_url: String,
    pub quic_port: u16,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let jwt_secret = env::var("JWT_SECRET")
            .map_err(|_| ConfigError::Missing("JWT_SECRET"))?;
        let rtmp_url = env::var("RTMP_URL")
            .unwrap_or_else(|_| "rtmp://media-server:1935/live".to_string());
        let quic_port: u16 = env::var("QUIC_PORT")
            .unwrap_or_else(|_| "4443".to_string())
            .parse()
            .unwrap_or(4443);

        Ok(Self {
            database_url,
            jwt_secret,
            rtmp_url,
            quic_port,
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
