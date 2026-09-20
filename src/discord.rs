//! Discord-specific command and response handling.

use std::{sync::atomic::Ordering, time::Instant};

use poise::serenity_prelude as serenity;
use tokio::sync::Semaphore;

use crate::{
    config::Config,
    decision::RawDecisionInput,
    jev::{JevClient, JevError},
    language::Language,
    render::{self, RenderedDecision, UserError},
};

mod flow;

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, Data, Error>;

struct Data {
    jev: JevClient,
    slots: Semaphore,
    guild_id: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum BotError {
    #[error("The Discord command definition could not be built.")]
    CommandDefinition,
    #[error("The Jev client could not be configured.")]
    JevConfiguration,
    #[error("Discord authentication failed. Check the bot token.")]
    Authentication,
    #[error("The guild command could not be registered. Check the server ID and bot installation.")]
    Registration,
    #[error("The Discord client could not be initialized.")]
    Client,
    #[error("The Discord Gateway connection failed.")]
    Gateway,
    #[error("The shutdown signal could not be initialized.")]
    Shutdown,
}

pub async fn run(config: Config) -> Result<(), BotError> {
    let data = Data {
        jev: JevClient::new(config.typesafe_api_key(), config.model())
            .map_err(|_| BotError::JevConfiguration)?,
        slots: Semaphore::new(2),
        guild_id: config.guild_id(),
    };

    // Upsert one command before starting the Gateway. A registration failure
    // must terminate startup, and reconnects must not repeat registration.
    let http = serenity::Http::new(config.discord_token());
    let application = http
        .get_current_application_info()
        .await
        .map_err(|_| BotError::Authentication)?;
    http.set_application_id(application.id);
    serenity::GuildId::new(config.guild_id())
        .create_command(&http, command_definition()?)
        .await
        .map_err(|_| BotError::Registration)?;
    tracing::info!("Registered the guild command");

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![jev()],
            allowed_mentions: Some(no_mentions()),
            initialize_owners: false,
            skip_checks_for_owners: false,
            on_error: |error| Box::pin(on_framework_error(error)),
            ..Default::default()
        })
        .setup(move |_, _, _| {
            Box::pin(async move {
                tracing::info!("Discord session is ready");
                Ok(data)
            })
        })
        .build();
    let mut client = serenity::ClientBuilder::new_with_http(http, serenity::GatewayIntents::GUILDS)
        .framework(framework)
        .await
        .map_err(|_| BotError::Client)?;
    let shard_manager = client.shard_manager.clone();

    tokio::select! {
        result = client.start() => result.map_err(|_| BotError::Gateway),
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|_| BotError::Shutdown)?;
            tracing::info!("Shutting down");
            shard_manager.shutdown_all().await;
            Ok(())
        }
    }
}

pub fn command_definition() -> Result<serenity::CreateCommand, BotError> {
    let mut registration = jev();
    // The guild endpoint supplies scope. Poise would otherwise serialize the
    // global-only `contexts` field; the separate framework command stays guild-only.
    registration.guild_only = false;
    registration
        .create_as_slash_command()
        .ok_or(BotError::CommandDefinition)
}

async fn configured_guild(ctx: Context<'_>) -> Result<bool, Error> {
    Ok(ctx.guild_id().map(|id| id.get()) == Some(ctx.data().guild_id))
}

/// Ask Jev to choose one of your options. The question, options, and context are public.
#[allow(clippy::too_many_arguments)]
#[poise::command(
    slash_command,
    guild_only,
    user_cooldown = 10,
    check = "configured_guild"
)]
async fn jev(
    ctx: Context<'_>,
    #[description = "The question to decide; sent to TypeSafe and shown publicly"]
    #[max_length = 300]
    question: String,
    #[description = "First option"]
    #[max_length = 120]
    a: String,
    #[description = "Second option"]
    #[max_length = 120]
    b: String,
    #[description = "Third option (optional)"]
    #[max_length = 120]
    c: Option<String>,
    #[description = "Fourth option (requires the third)"]
    #[max_length = 120]
    d: Option<String>,
    #[description = "Additional facts; sent to TypeSafe and shown publicly"]
    #[max_length = 500]
    context: Option<String>,
) -> Result<(), Error> {
    let poise::Context::Application(app) = ctx else {
        return Ok(());
    };
    let language = Language::from_question(&question);
    let raw = RawDecisionInput {
        question,
        a,
        b,
        c,
        d,
        context,
    };
    let (request, permit) = match flow::admit(
        ctx.guild_id().map(|id| id.get()),
        ctx.data().guild_id,
        raw,
        &ctx.data().slots,
    ) {
        Ok(admitted) => admitted,
        Err(rejection) => {
            let message = match rejection {
                flow::Rejection::Input(error) => render::input_error(&error, language),
                flow::Rejection::WrongGuild => render::error(UserError::WrongGuild, language),
                flow::Rejection::Busy => render::error(UserError::Busy, language),
            };
            respond_error(app, message).await;
            return Ok(());
        }
    };

    let started = Instant::now();
    let execution_id = app.interaction.id.get();
    tracing::info!(
        execution_id,
        option_count = request.options().len(),
        "Evaluation started"
    );
    let delivery = flow::run_deferred(
        permit,
        async {
            app.interaction
                .create_response(ctx.http(), deferred_response())
                .await
                .map_err(|_| ())?;
            app.has_sent_initial_response.store(true, Ordering::SeqCst);
            Ok(())
        },
        ctx.data().jev.decide(&request),
        |outcome| async {
            let edit = match outcome {
                Ok(result) => {
                    // Debug string formatting escapes provider-controlled log characters.
                    tracing::info!(
                        execution_id,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        model = ?result.model,
                        input_tokens = result.usage.input_tokens,
                        output_tokens = result.usage.output_tokens,
                        "Evaluation completed",
                    );
                    match render::decision(&request, &result, language) {
                        Ok(rendered) => result_edit(rendered),
                        Err(_) => {
                            tracing::warn!(
                                execution_id,
                                error = "display",
                                "Result could not be rendered"
                            );
                            error_edit(render::error(UserError::Display, language))
                        }
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        execution_id,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        error = ?error,
                        "Evaluation failed",
                    );
                    error_edit(render::error(user_error(error), language))
                }
            };
            app.interaction
                .edit_response(ctx.http(), edit)
                .await
                .map(|_| ())
                .map_err(|_| ())
        },
    )
    .await;
    if let Err(stage) = delivery {
        tracing::warn!(execution_id, stage = ?stage, "Discord delivery failed");
    }
    // Delivery and evaluation errors are handled here; do not ask the
    // framework to send another response or re-run an evaluation.
    Ok(())
}

fn user_error(error: JevError) -> UserError {
    match error {
        JevError::Authentication | JevError::Configuration => UserError::Authentication,
        JevError::InvalidRequest => UserError::InvalidRequest,
        JevError::RateLimited => UserError::RateLimited,
        JevError::Overloaded => UserError::Overloaded,
        JevError::Timeout => UserError::Timeout,
        JevError::InvalidResponse => UserError::InvalidResponse,
        JevError::Transport => UserError::Transport,
    }
}

fn question_language(args: &[serenity::ResolvedOption<'_>]) -> Language {
    args.iter()
        .find_map(|option| {
            if option.name == "question"
                && let serenity::ResolvedValue::String(question) = &option.value
            {
                return Some(Language::from_question(question));
            }
            None
        })
        .unwrap_or(Language::English)
}

async fn on_framework_error(error: poise::FrameworkError<'_, Data, Error>) {
    let category = match &error {
        poise::FrameworkError::CooldownHit { .. } => UserError::Cooldown,
        poise::FrameworkError::GuildOnly { .. }
        | poise::FrameworkError::CommandCheckFailed { .. } => UserError::WrongGuild,
        _ => UserError::Internal,
    };
    // Framework errors can contain complete inputs and interactions. Never
    // format the error, even when the operator enables verbose logging.
    if let Some(poise::Context::Application(app)) = error.ctx() {
        tracing::warn!(execution_id = app.interaction.id.get(), error = ?category, "Command rejected");
        let message = render::error(category, question_language(app.args));
        respond_error(app, message).await;
    } else {
        tracing::warn!(error = "framework", "Discord framework error");
    }
}

async fn respond_error(app: poise::ApplicationContext<'_, Data, Error>, message: String) {
    let result = if app.has_sent_initial_response.load(Ordering::SeqCst) {
        app.interaction
            .edit_response(app.framework.serenity_context, error_edit(message))
            .await
            .map(|_| ())
    } else {
        let result = app
            .interaction
            .create_response(app.framework.serenity_context, private_response(message))
            .await;
        if result.is_ok() {
            app.has_sent_initial_response.store(true, Ordering::SeqCst);
        }
        result
    };
    if result.is_err() {
        tracing::warn!(
            execution_id = app.interaction.id.get(),
            error = "discord_reply",
            "Error reply failed"
        );
    }
}

fn no_mentions() -> serenity::CreateAllowedMentions {
    serenity::CreateAllowedMentions::new()
        .all_users(false)
        .all_roles(false)
        .everyone(false)
        .empty_users()
        .empty_roles()
        .replied_user(false)
}

fn deferred_response() -> serenity::CreateInteractionResponse {
    serenity::CreateInteractionResponse::Defer(
        serenity::CreateInteractionResponseMessage::new()
            .ephemeral(false)
            .allowed_mentions(no_mentions()),
    )
}

fn private_response(content: String) -> serenity::CreateInteractionResponse {
    serenity::CreateInteractionResponse::Message(
        serenity::CreateInteractionResponseMessage::new()
            .content(content)
            .ephemeral(true)
            .allowed_mentions(no_mentions()),
    )
}

fn error_edit(content: String) -> serenity::EditInteractionResponse {
    serenity::EditInteractionResponse::new()
        .content(content)
        .embeds(Vec::new())
        .allowed_mentions(no_mentions())
}

fn result_edit(rendered: RenderedDecision) -> serenity::EditInteractionResponse {
    let mut embed = serenity::CreateEmbed::new()
        .title(rendered.title)
        .description(rendered.description)
        .footer(serenity::CreateEmbedFooter::new(rendered.footer));
    for field in rendered.fields {
        embed = embed.field(field.name, field.value, false);
    }
    serenity::EditInteractionResponse::new()
        .content("")
        .embed(embed)
        .allowed_mentions(no_mentions())
}

#[cfg(test)]
mod tests;
