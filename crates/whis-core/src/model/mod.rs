//! Local model catalog, download, and verification.
//!
//! Models are described as data in [`catalog`]; the same list drives the
//! desktop UI, the CLI, and transcription dispatch. Downloading and removing
//! models lives in [`install`].

pub mod catalog;
pub mod install;

pub use catalog::{
    DEFAULT_MODEL_ID, LocalModelSpec, ModelEngine, ModelFormat, catalog as all_models, find,
    id_for_path, models_dir,
};
