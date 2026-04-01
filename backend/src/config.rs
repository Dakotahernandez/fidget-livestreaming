use std::env;

/// Application configuration loaded from environment variables.
#[derive(Clone, Debug)]
pub struct Config {
    pub database_url: String,
    pub jwt_secret: String,
}

impl Config {
    /// Build a `Config` from environment variables.
    ///
    /// Required vars: `DATABASE_URL`, `JWT_SECRET`.
    pub fn from_env() -> Result<Self, ConfigError> {
        let database_url = env::var("DATABASE_URL")
            .map_err(|_| ConfigError::Missing("DATABASE_URL"))?;
        let jwt_secret = env::var("JWT_SECRET")
            .map_err(|_| ConfigError::Missing("JWT_SECRET"))?;

        Ok(Self {
            database_url,
            jwt_secret,
        })
    }
}

/// Errors that can occur while loading configuration.
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
