//! Model listing commands for local models and ollama

use anyhow::{Context, Result};
use serde::Deserialize;
use std::time::Duration;
use whis_core::model::{self, ModelEngine};
use whis_core::ollama;

use crate::args::{ModelAction, ModelType as ModelTypeArg};

/// Run the model command
pub fn run(action: Option<ModelAction>) -> Result<()> {
    match action {
        None | Some(ModelAction::List { model_type: None }) => list_all_local_models(),
        Some(ModelAction::List {
            model_type: Some(ModelTypeArg::Whisper),
        }) => list_engine_models(ModelEngine::WhisperCpp, "whisper"),
        Some(ModelAction::List {
            model_type: Some(ModelTypeArg::Parakeet),
        }) => list_engine_models(ModelEngine::Onnx, "Parakeet"),
        Some(ModelAction::List {
            model_type: Some(ModelTypeArg::Ollama { url }),
        }) => list_ollama_models(url),
    }
}

/// List every local model, grouped by engine.
fn list_all_local_models() -> Result<()> {
    list_engine_models(ModelEngine::WhisperCpp, "whisper")?;
    list_engine_models(ModelEngine::Onnx, "Parakeet")?;
    list_engine_models(ModelEngine::Gguf, "GGUF")?;
    print_models_hint();
    Ok(())
}

/// List the models of one engine with install status.
fn list_engine_models(engine: ModelEngine, label: &str) -> Result<()> {
    println!("Available {label} models:\n");

    let models: Vec<_> = model::all_models()
        .iter()
        .filter(|m| m.engine == engine)
        .collect();

    let name_width = models
        .iter()
        .map(|m| m.name.len())
        .max()
        .unwrap_or(6)
        .max(4);

    println!(
        "{:<name_width$}  STATUS       DESCRIPTION",
        "NAME",
        name_width = name_width
    );
    println!("{}", "-".repeat(70));

    for m in models {
        let status = if m.is_installed() { "[installed]" } else { "" };
        println!(
            "{:<name_width$}  {:<11}  {} ({})",
            m.name,
            status,
            m.description,
            m.size_label(),
            name_width = name_width
        );
    }

    println!();

    Ok(())
}

fn print_models_hint() {
    println!("Models directory: {}", model::models_dir().display());
    println!();
    println!("To download a model, run: whis setup local");
}

/// Response from Ollama /api/tags endpoint
#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<OllamaModel>,
}

/// Model info from Ollama
#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
    #[serde(default)]
    size: u64,
}

/// List available Ollama models from server
fn list_ollama_models(url: Option<String>) -> Result<()> {
    let url = url.as_deref().unwrap_or(ollama::DEFAULT_OLLAMA_URL);

    // Check if Ollama is running
    if !ollama::is_ollama_running(url).unwrap_or(false) {
        println!("Ollama is not running at {}\n", url);
        println!("Start Ollama with: ollama serve");
        println!("Or specify a different URL: whis model list ollama --url http://...");
        return Ok(());
    }

    // Fetch models from Ollama
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("Failed to create HTTP client")?;

    let tags_url = format!("{}/api/tags", url.trim_end_matches('/'));
    let response = client
        .get(&tags_url)
        .send()
        .context("Failed to connect to Ollama")?;

    if !response.status().is_success() {
        anyhow::bail!("Ollama returned error: {}", response.status());
    }

    let tags: TagsResponse = response.json().context("Failed to parse Ollama response")?;

    if tags.models.is_empty() {
        println!("No models installed in Ollama at {}\n", url);
        println!("Pull a model with: ollama pull <model>");
        println!("Example: ollama pull qwen2.5:1.5b");
        return Ok(());
    }

    println!("Ollama models at {}:\n", url);

    // Calculate column widths
    let name_width = tags
        .models
        .iter()
        .map(|m| m.name.len())
        .max()
        .unwrap_or(4)
        .max(4);

    // Print header
    println!("{:<name_width$}  SIZE", "NAME", name_width = name_width);
    println!("{}", "-".repeat(name_width + 12));

    // Print each model
    for model in &tags.models {
        println!(
            "{:<name_width$}  {}",
            model.name,
            format_size(model.size),
            name_width = name_width
        );
    }

    Ok(())
}

/// Format bytes as human-readable size
fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return String::new();
    }

    if bytes >= 1_000_000_000 {
        format!("{:.1} GB", bytes as f64 / 1_000_000_000.0)
    } else if bytes >= 1_000_000 {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    } else if bytes >= 1_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else {
        format!("{} B", bytes)
    }
}
