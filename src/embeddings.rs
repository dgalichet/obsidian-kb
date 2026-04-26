use anyhow::{Context, Result, bail};
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use std::str::FromStr;

use crate::config::AppConfig;

pub struct FastEmbedder {
    model: TextEmbedding,
    model_name: EmbeddingModel,
    batch_size: usize,
    normalize: bool,
}

impl FastEmbedder {
    pub fn new(config: &AppConfig) -> Result<Self> {
        if !config.embeddings.provider.eq_ignore_ascii_case("fastembed") {
            bail!(
                "unsupported embedding provider `{}`; only `fastembed` is supported",
                config.embeddings.provider
            );
        }

        let model_name = parse_embedding_model(&config.embeddings.model)?;
        let cache_dir = config.embedding_cache_dir();
        std::fs::create_dir_all(&cache_dir)?;
        let options = TextInitOptions::new(model_name.clone())
            .with_cache_dir(cache_dir)
            .with_show_download_progress(true);
        let model = TextEmbedding::try_new(options)
            .with_context(|| format!("failed to initialize fastembed model `{model_name}`"))?;

        Ok(Self {
            model,
            model_name,
            batch_size: config.embeddings.batch_size.max(1),
            normalize: config.embeddings.normalize,
        })
    }

    pub fn model_key(&self) -> String {
        format!("fastembed:{}", self.model_name)
    }

    pub fn embed_passages(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.embed_prefixed(texts, PrefixKind::Passage)
    }

    pub fn embed_query(&mut self, query: &str) -> Result<Vec<f32>> {
        let embeddings = self.embed_prefixed(&[query.to_string()], PrefixKind::Query)?;
        embeddings
            .into_iter()
            .next()
            .context("fastembed returned no query embedding")
    }

    fn embed_prefixed(&mut self, texts: &[String], kind: PrefixKind) -> Result<Vec<Vec<f32>>> {
        let prefixed = texts
            .iter()
            .map(|text| prefix_text(&self.model_name, text, kind))
            .collect::<Vec<_>>();
        let mut embeddings = self.model.embed(prefixed, Some(self.batch_size))?;
        if self.normalize {
            for embedding in &mut embeddings {
                normalize(embedding);
            }
        }
        Ok(embeddings)
    }
}

#[derive(Debug, Clone, Copy)]
enum PrefixKind {
    Passage,
    Query,
}

pub fn parse_embedding_model(value: &str) -> Result<EmbeddingModel> {
    EmbeddingModel::from_str(value).map_err(|message| anyhow::anyhow!(message))
}

pub fn embedding_dimension(value: &str) -> Result<usize> {
    let model = parse_embedding_model(value)?;
    Ok(TextEmbedding::get_model_info(&model)?.dim)
}

pub fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for value in vector {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

pub fn cosine(left: &[f32], right: &[f32]) -> f32 {
    let mut dot = 0.0;
    let mut left_norm = 0.0;
    let mut right_norm = 0.0;
    for (left, right) in left.iter().zip(right) {
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        0.0
    } else {
        dot / (left_norm.sqrt() * right_norm.sqrt())
    }
}

fn prefix_text(model: &EmbeddingModel, text: &str, kind: PrefixKind) -> String {
    if !is_e5_like(model) {
        return text.to_string();
    }
    let trimmed = text.trim();
    match kind {
        PrefixKind::Passage if has_prefix(trimmed, "passage:") => trimmed.to_string(),
        PrefixKind::Query if has_prefix(trimmed, "query:") => trimmed.to_string(),
        PrefixKind::Passage => format!("passage: {trimmed}"),
        PrefixKind::Query => format!("query: {trimmed}"),
    }
}

fn is_e5_like(model: &EmbeddingModel) -> bool {
    matches!(
        model,
        EmbeddingModel::MultilingualE5Small
            | EmbeddingModel::MultilingualE5Base
            | EmbeddingModel::MultilingualE5Large
    )
}

fn has_prefix(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .map(|value| value.eq_ignore_ascii_case(prefix))
        .unwrap_or(false)
}

fn normalize(vector: &mut [f32]) {
    let magnitude = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if magnitude > 0.0 {
        for value in vector {
            *value /= magnitude;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_configured_fastembed_models() {
        for model in [
            "MultilingualE5Small",
            "ParaphraseMLMiniLML12V2",
            "ParaphraseMLMiniLML12V2Q",
            "BGEM3",
            "MultilingualE5Base",
            "MultilingualE5Large",
        ] {
            assert!(parse_embedding_model(model).is_ok(), "{model}");
            assert!(embedding_dimension(model).unwrap() > 0, "{model}");
        }
    }

    #[test]
    fn prefixes_e5_passages_and_queries() {
        let model = EmbeddingModel::MultilingualE5Small;
        assert_eq!(
            prefix_text(&model, "bonjour le monde", PrefixKind::Passage),
            "passage: bonjour le monde"
        );
        assert_eq!(
            prefix_text(&model, "retrieval query", PrefixKind::Query),
            "query: retrieval query"
        );
        assert_eq!(
            prefix_text(&model, "passage: already prefixed", PrefixKind::Passage),
            "passage: already prefixed"
        );
    }

    #[test]
    fn does_not_prefix_non_e5_models() {
        let model = EmbeddingModel::BGEM3;
        assert_eq!(
            prefix_text(&model, "plain text", PrefixKind::Passage),
            "plain text"
        );
    }
}
