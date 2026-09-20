use std::collections::BTreeMap;
use std::env;

use thiserror::Error;

const DEFAULT_MODEL: &str = "jev-latest";
const DEFAULT_LOG_FILTER: &str = "warn,jev_pick=info";
const CONFIG_KEYS: [ConfigKey; 5] = [
    ConfigKey::DiscordToken,
    ConfigKey::TypesafeApiKey,
    ConfigKey::DiscordGuildId,
    ConfigKey::JevModel,
    ConfigKey::RustLog,
];

pub struct Config {
    discord_token: String,
    typesafe_api_key: String,
    guild_id: u64,
    model: String,
    log_filter: String,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();

        match dotenvy::dotenv_iter() {
            Ok(entries) => {
                for entry in entries {
                    let (key, value) = entry.map_err(classify_dotenv_error)?;
                    values.entry(key).or_insert(value);
                }
            }
            Err(error) if error.not_found() => {}
            Err(_) => return Err(ConfigError::DotenvUnavailable),
        }

        for key in CONFIG_KEYS {
            match env::var(key.as_str()) {
                Ok(value) => {
                    values.insert(key.as_str().to_owned(), value);
                }
                Err(env::VarError::NotPresent) => {}
                Err(env::VarError::NotUnicode(_)) => {
                    return Err(ConfigError::InvalidUnicode { key });
                }
            }
        }

        Self::from_values(values)
    }

    #[doc(hidden)]
    pub fn from_sources<I, K, V>(
        dotenv_contents: Option<&str>,
        environment: I,
    ) -> Result<Self, ConfigError>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let mut values = BTreeMap::new();

        if let Some(contents) = dotenv_contents {
            for entry in dotenvy::from_read_iter(contents.as_bytes()) {
                let (key, value) = entry.map_err(|_| ConfigError::MalformedDotenv)?;
                values.entry(key).or_insert(value);
            }
        }

        for (key, value) in environment {
            values.insert(key.as_ref().to_owned(), value.as_ref().to_owned());
        }

        Self::from_values(values)
    }

    fn from_values(mut values: BTreeMap<String, String>) -> Result<Self, ConfigError> {
        let discord_token = take_required(&mut values, ConfigKey::DiscordToken)?;
        let typesafe_api_key = take_required(&mut values, ConfigKey::TypesafeApiKey)?;
        let guild_id_text = take_required(&mut values, ConfigKey::DiscordGuildId)?;
        let guild_id = guild_id_text
            .parse::<u64>()
            .ok()
            .filter(|value| *value != 0)
            .ok_or(ConfigError::InvalidGuildId)?;

        let model = match values.remove(ConfigKey::JevModel.as_str()) {
            Some(value) if value.trim().is_empty() => {
                return Err(ConfigError::Empty {
                    key: ConfigKey::JevModel,
                });
            }
            Some(value) => value,
            None => DEFAULT_MODEL.to_owned(),
        };
        let log_filter = values
            .remove(ConfigKey::RustLog.as_str())
            .unwrap_or_else(|| DEFAULT_LOG_FILTER.to_owned());

        Ok(Self {
            discord_token,
            typesafe_api_key,
            guild_id,
            model,
            log_filter,
        })
    }

    pub fn discord_token(&self) -> &str {
        &self.discord_token
    }

    pub fn typesafe_api_key(&self) -> &str {
        &self.typesafe_api_key
    }

    pub fn guild_id(&self) -> u64 {
        self.guild_id
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn log_filter(&self) -> &str {
        &self.log_filter
    }
}

fn take_required(
    values: &mut BTreeMap<String, String>,
    key: ConfigKey,
) -> Result<String, ConfigError> {
    let value = values
        .remove(key.as_str())
        .ok_or(ConfigError::Missing { key })?;
    if value.trim().is_empty() {
        return Err(ConfigError::Empty { key });
    }
    Ok(value)
}

fn classify_dotenv_error(error: dotenvy::Error) -> ConfigError {
    match error {
        dotenvy::Error::LineParse(_, _) => ConfigError::MalformedDotenv,
        _ => ConfigError::DotenvUnavailable,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConfigKey {
    DiscordToken,
    TypesafeApiKey,
    DiscordGuildId,
    JevModel,
    RustLog,
}

impl ConfigKey {
    fn as_str(self) -> &'static str {
        match self {
            Self::DiscordToken => "DISCORD_TOKEN",
            Self::TypesafeApiKey => "TYPESAFE_API_KEY",
            Self::DiscordGuildId => "DISCORD_GUILD_ID",
            Self::JevModel => "JEV_MODEL",
            Self::RustLog => "RUST_LOG",
        }
    }
}

impl std::fmt::Display for ConfigKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("required configuration {key} is missing")]
    Missing { key: ConfigKey },
    #[error("configuration {key} must not be empty")]
    Empty { key: ConfigKey },
    #[error("DISCORD_GUILD_ID must be a nonzero unsigned integer")]
    InvalidGuildId,
    #[error("configuration {key} is not valid Unicode")]
    InvalidUnicode { key: ConfigKey },
    #[error("the .env file is malformed")]
    MalformedDotenv,
    #[error("the .env file could not be read")]
    DotenvUnavailable,
}
