mod logging;

use std::process::ExitCode;

use jev_pick::{config::Config, discord};

fn main() -> ExitCode {
    match start() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Every propagated error is an application-owned, value-free type.
            eprintln!("JevPick: {error}");
            ExitCode::FAILURE
        }
    }
}

fn start() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    logging::init(config.log_filter())?;
    let runtime = tokio::runtime::Runtime::new().map_err(|_| RuntimeError)?;
    runtime.block_on(discord::run(config))?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("The asynchronous runtime could not be initialized.")]
struct RuntimeError;
