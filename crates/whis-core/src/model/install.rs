//! Download, extract, and remove catalog models.

use super::catalog::{self, LocalModelSpec, ModelFormat};
use anyhow::{Context, Result, anyhow, bail};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

/// Install a model, reporting progress as `(downloaded, total)`.
///
/// Returns the model path. An already installed model is returned without
/// touching the network.
pub fn install_with_progress<F>(model: &LocalModelSpec, on_progress: F) -> Result<PathBuf>
where
    F: Fn(u64, u64),
{
    if !model.supported {
        bail!("{} is not a supported transcription model", model.name);
    }
    if !model.engine.is_built() {
        bail!(
            "{} requires the '{}' engine, which is not included in this build",
            model.name,
            model.engine.as_str()
        );
    }
    if model.url.is_empty() {
        bail!("{} has no download source", model.name);
    }

    let dest = model.path();
    if model.verify(&dest) {
        return Ok(dest);
    }

    let parent = dest
        .parent()
        .ok_or_else(|| anyhow!("Model path has no parent directory"))?;
    fs::create_dir_all(parent).context("Failed to create models directory")?;

    let temp_path = temp_path(model, &dest);
    // A leftover partial download would otherwise corrupt the next attempt.
    fs::remove_file(&temp_path).ok();

    if let Err(e) = download_file(model.url, &temp_path, &on_progress) {
        fs::remove_file(&temp_path).ok();
        return Err(e);
    }

    let result = match model.format {
        ModelFormat::OnnxArchive => extract_archive(model, &temp_path, parent),
        _ => fs::rename(&temp_path, &dest).context("Failed to finalize model download"),
    };
    fs::remove_file(&temp_path).ok();

    result?;

    if !model.verify(&dest) {
        bail!("Downloaded {} but the files are incomplete", model.name);
    }

    Ok(dest)
}

/// Install a model with a simple progress bar on stderr.
pub fn install(model: &LocalModelSpec) -> Result<PathBuf> {
    install_with_progress(model, |downloaded, total| {
        let percent = downloaded
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or(0);
        eprint!("\r[{}%] {:.1} MB", percent, downloaded as f64 / 1_000_000.0);
        std::io::stderr().flush().ok();
    })
    .inspect(|_| eprintln!())
}

/// Delete an installed model.
pub fn remove(model: &LocalModelSpec) -> Result<()> {
    let path = model.path();
    if !path.exists() {
        return Ok(());
    }
    if !path.starts_with(catalog::models_dir()) {
        bail!("Refusing to delete a path outside the models directory");
    }

    match model.format {
        ModelFormat::OnnxArchive => fs::remove_dir_all(&path)
            .with_context(|| format!("Failed to delete {}", path.display())),
        ModelFormat::None => Ok(()),
        _ => fs::remove_file(&path).with_context(|| format!("Failed to delete {}", path.display())),
    }
}

fn temp_path(model: &LocalModelSpec, dest: &std::path::Path) -> PathBuf {
    match model.format {
        ModelFormat::OnnxArchive => {
            dest.with_file_name(format!(".{}.tar.gz.part", model.file_name))
        }
        _ => {
            let name = dest
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| model.id.to_string());
            dest.with_file_name(format!("{name}.part"))
        }
    }
}

fn download_file<F>(url: &str, dest: &std::path::Path, on_progress: &F) -> Result<()>
where
    F: Fn(u64, u64),
{
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3600))
        .build()
        .context("Failed to create HTTP client")?;

    let mut response = client
        .get(url)
        .send()
        .context("Failed to start model download")?;

    if !response.status().is_success() {
        bail!("Download failed: HTTP {}", response.status());
    }

    let total = response.content_length().unwrap_or(0);
    let mut file = File::create(dest).context("Failed to create download file")?;
    let mut buffer = [0u8; 64 * 1024];
    let mut downloaded = 0u64;
    let mut reported = 0u64;

    on_progress(0, total);
    loop {
        let read = response.read(&mut buffer).context("Download interrupted")?;
        if read == 0 {
            break;
        }
        file.write_all(&buffer[..read])
            .context("Failed to write model file")?;
        downloaded += read as u64;

        // Report often enough for a smooth bar without flooding the UI.
        let step = if total > 0 {
            (total / 100).max(256 * 1024)
        } else {
            1024 * 1024
        };
        if downloaded - reported >= step {
            on_progress(downloaded, total);
            reported = downloaded;
        }
    }

    file.flush().context("Failed to flush model file")?;
    on_progress(downloaded, total);
    Ok(())
}

#[cfg(feature = "local-transcription")]
fn extract_archive(
    model: &LocalModelSpec,
    archive: &std::path::Path,
    parent: &std::path::Path,
) -> Result<()> {
    use flate2::read::GzDecoder;
    use tar::Archive;

    let file = File::open(archive).context("Failed to open downloaded archive")?;
    let mut tar = Archive::new(GzDecoder::new(file));
    tar.unpack(parent)
        .with_context(|| format!("Failed to extract {}", model.name))?;
    Ok(())
}

#[cfg(not(feature = "local-transcription"))]
fn extract_archive(
    _model: &LocalModelSpec,
    _archive: &std::path::Path,
    _parent: &std::path::Path,
) -> Result<()> {
    bail!("Archive extraction is not enabled in this build")
}
