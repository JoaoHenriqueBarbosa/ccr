mod api;
mod auth;
mod tools;
mod tui;
mod types;

use types::ModelId;

#[tokio::main]
async fn main() -> std::result::Result<(), types::AppError> {
    let api_key = auth::resolve_api_key()?;

    let model = std::env::var("CLAUDE_MODEL")
        .unwrap_or_else(|_| "claude-opus-4-6".into());

    tui::run(api_key, ModelId::new(model))
}
