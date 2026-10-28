//! Local (on-device) transcription setup
//!
//! Lists the downloadable models from the shared catalog, downloads the
//! selected one, and stores the selection in settings.

use anyhow::{Result, bail};
use whis_core::{Settings, TranscriptionProvider, model};

use super::interactive;

/// Streamlined local transcription setup (no post-processing config)
/// Used by the unified wizard
pub fn setup_transcription_local() -> Result<()> {
    let mut settings = Settings::load_cli();

    let current_id = settings
        .transcription
        .local_models
        .selected
        .clone()
        .or_else(|| {
            settings
                .transcription
                .local_model_path()
                .as_deref()
                .and_then(model::id_for_path)
                .map(str::to_string)
        });

    let models: Vec<&model::LocalModelSpec> = model::all_models()
        .iter()
        .filter(|m| m.is_available())
        .collect();

    if models.is_empty() {
        bail!("No local models are available in this build");
    }

    let items: Vec<String> = models
        .iter()
        .map(|m| {
            let installed = if m.is_installed() { " [installed]" } else { "" };
            let current = if current_id.as_deref() == Some(m.id) {
                " [current]"
            } else {
                ""
            };
            format!(
                "{} - {} ({}){}{}",
                m.name,
                m.engine.label(),
                m.size_label(),
                installed,
                current
            )
        })
        .collect();
    let clean_items: Vec<String> = models.iter().map(|m| m.name.to_string()).collect();

    let default = models
        .iter()
        .position(|m| Some(m.id) == current_id.as_deref())
        .unwrap_or(0);

    let choice =
        interactive::select_clean("Which local model?", &items, &clean_items, Some(default))?;
    let selected = models[choice];

    if !selected.is_installed() {
        interactive::info(&format!("Downloading {}...", selected.name));
        model::install::install(selected)?;
    }

    settings.transcription.provider = TranscriptionProvider::Local;
    settings.transcription.local_models.selected = Some(selected.id.to_string());
    settings.transcription.local_models.custom_path = None;
    settings.save_cli()?;

    interactive::info(&format!("Using {} for local transcription.", selected.name));
    Ok(())
}
