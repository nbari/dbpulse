use super::{commands, dispatch};
use anyhow::Result;

/// Main orchestrator - Pure orchestration with no business logic
///
/// Five-step data flow:
/// 1. Parse: Extract CLI arguments
/// 2. Extract Verbosity: Convert flag count to logging level (not used here)
/// 3. Initialize Telemetry: Set up structured logging/tracing (not used here)
/// 4. Dispatch: Convert `ArgMatches` into typed Action enum
/// 5. Execute: Run the action's business logic
///
/// # Errors
///
/// Returns an error if any step in the flow fails
pub async fn start() -> Result<()> {
    // 1. Parse: Extract CLI arguments
    let matches = commands::new().get_matches();

    // 2. Extract Verbosity (optional - not used in dbpulse)
    // let verbosity = extract_verbosity(&matches);

    // 3. Initialize Telemetry (optional - can be added later)
    // telemetry::init(verbosity)?;

    // 4. Dispatch: Convert ArgMatches into typed Action enum
    let action = dispatch::dispatch(matches)?;

    // 5. Execute: Run the action's business logic
    action.execute().await?;

    Ok(())
}
