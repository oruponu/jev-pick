use jev_pick::config::{Config, ConfigError, ConfigKey};

fn required_env() -> Vec<(&'static str, &'static str)> {
    vec![
        ("DISCORD_TOKEN", "environment-discord-token"),
        ("TYPESAFE_API_KEY", "environment-api-key"),
        ("DISCORD_GUILD_ID", "42"),
    ]
}

#[test]
fn required_values_and_defaults_are_loaded() {
    let config = Config::from_sources(None, required_env()).unwrap();

    assert_eq!(config.discord_token(), "environment-discord-token");
    assert_eq!(config.typesafe_api_key(), "environment-api-key");
    assert_eq!(config.guild_id(), 42);
    assert_eq!(config.model(), "jev-latest");
    assert_eq!(config.log_filter(), "warn,jev_pick=info");
}

#[test]
fn dotenv_is_parsed_without_mutating_environment_and_environment_wins() {
    let dotenv = "DISCORD_TOKEN=file-discord-token
TYPESAFE_API_KEY=file-api-key
DISCORD_GUILD_ID=7
JEV_MODEL=file-model
RUST_LOG=info
";
    let env = [
        ("DISCORD_TOKEN", "environment-discord-token"),
        ("DISCORD_GUILD_ID", "99"),
        ("JEV_MODEL", "environment-model"),
    ];

    let config = Config::from_sources(Some(dotenv), env).unwrap();

    assert_eq!(config.discord_token(), "environment-discord-token");
    assert_eq!(config.typesafe_api_key(), "file-api-key");
    assert_eq!(config.guild_id(), 99);
    assert_eq!(config.model(), "environment-model");
    assert_eq!(config.log_filter(), "info");
}

#[test]
fn missing_and_empty_required_values_are_distinct_typed_errors() {
    let missing = Config::from_sources(
        None,
        [("TYPESAFE_API_KEY", "api-key"), ("DISCORD_GUILD_ID", "42")],
    )
    .err()
    .expect("missing token must fail");
    assert_eq!(
        missing,
        ConfigError::Missing {
            key: ConfigKey::DiscordToken,
        }
    );

    let empty = Config::from_sources(
        None,
        [
            ("DISCORD_TOKEN", "   "),
            ("TYPESAFE_API_KEY", "api-key"),
            ("DISCORD_GUILD_ID", "42"),
        ],
    )
    .err()
    .expect("blank token must fail");
    assert_eq!(
        empty,
        ConfigError::Empty {
            key: ConfigKey::DiscordToken,
        }
    );
}

#[test]
fn guild_id_must_be_a_nonzero_u64() {
    for value in ["0", "-1", "not-a-number", "18446744073709551616"] {
        let mut env = required_env();
        env[2] = ("DISCORD_GUILD_ID", value);
        let error = Config::from_sources(None, env)
            .err()
            .expect("invalid guild ID must fail");
        assert_eq!(error, ConfigError::InvalidGuildId);
    }
}

#[test]
fn explicitly_empty_model_is_invalid_while_missing_model_defaults() {
    let mut env = required_env();
    env.push(("JEV_MODEL", " \t "));

    let error = Config::from_sources(None, env)
        .err()
        .expect("blank model must fail");
    assert_eq!(
        error,
        ConfigError::Empty {
            key: ConfigKey::JevModel,
        }
    );
}

#[test]
fn malformed_dotenv_is_rejected_without_exposing_its_line_or_value() {
    let secret = "secret-value-that-must-not-leak";
    let dotenv = format!("DISCORD_TOKEN=\"{secret}");

    let error = Config::from_sources(Some(&dotenv), required_env())
        .err()
        .expect("malformed dotenv must fail");

    assert_eq!(error, ConfigError::MalformedDotenv);
    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}

#[test]
fn config_errors_never_include_invalid_values() {
    let secret = "invalid-secret-guild-id";
    let env = [
        ("DISCORD_TOKEN", "token"),
        ("TYPESAFE_API_KEY", "key"),
        ("DISCORD_GUILD_ID", secret),
    ];

    let error = Config::from_sources(None, env)
        .err()
        .expect("invalid guild ID must fail");

    assert!(!error.to_string().contains(secret));
    assert!(!format!("{error:?}").contains(secret));
}
