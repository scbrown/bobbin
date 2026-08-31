//! ONNX cross-encoder implementation of the [`Reranker`](super::rerank::Reranker) seam.
//!
//! Mirrors the embedder's ort/tokenizer machinery (`index/embedder.rs`)
//! with the two differences a cross-encoder brings: the tokenizer encodes
//! (query, passage) PAIRS, and the model emits a single relevance logit
//! per pair instead of token embeddings — so there is no pooling, just a
//! `[batch, 1]` (or `[batch]`) logits read.
//!
//! Model files are user-supplied (no download path); `token_type_ids` is
//! passed only when the model's input signature declares it, since
//! RoBERTa-style cross-encoders do not take it.
//!
//! HONESTY NOTE: this path compiles and is exercised at the seam by the
//! deterministic tests in `rerank_tests.rs`, but has not been validated
//! against a real cross-encoder model in CI (no model download exists).
//! Treat quality claims as unmeasured until the eval harness runs it.

use anyhow::{Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use std::sync::Mutex;
use tokenizers::Tokenizer;

use super::rerank::{Reranker, RerankerConfig};

pub struct OnnxCrossEncoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    max_seq: usize,
    /// Whether the model's input signature includes token_type_ids.
    wants_token_type_ids: bool,
    /// First declared output name — cross-encoder exports differ
    /// ("logits" usually, but not always), so read what the model says.
    output_name: String,
}

impl OnnxCrossEncoder {
    /// Load the cross-encoder from user-supplied files. CPU-only: the
    /// stage scores at most top-K pairs per query, which does not justify
    /// the GPU provider plumbing the bulk embedder needs.
    pub fn load(cfg: &RerankerConfig) -> Result<Self> {
        crate::index::embedder::auto_resolve_ort_dylib(false);

        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {e}"))?
            .commit_from_file(&cfg.model_path)
            .with_context(|| {
                format!("Failed to load reranker ONNX model from {}", cfg.model_path)
            })?;

        let wants_token_type_ids = session
            .inputs()
            .iter()
            .any(|input| input.name() == "token_type_ids");
        let output_name = session
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .ok_or_else(|| anyhow::anyhow!("reranker model declares no outputs"))?;

        let tokenizer = Tokenizer::from_file(&cfg.tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load reranker tokenizer: {e}"))?;

        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            max_seq: cfg.max_seq_len.max(8),
            wants_token_type_ids,
            output_name,
        })
    }
}

impl Reranker for OnnxCrossEncoder {
    fn score(&self, query: &str, passages: &[&str]) -> Result<Vec<f32>> {
        if passages.is_empty() {
            return Ok(vec![]);
        }

        // Cross-encoders read the pair jointly: encode (query, passage).
        let pairs: Vec<(String, String)> = passages
            .iter()
            .map(|p| (query.to_string(), p.to_string()))
            .collect();
        let encodings = self
            .tokenizer
            .encode_batch(pairs, true)
            .map_err(|e| anyhow::anyhow!("Reranker tokenization failed: {e}"))?;

        let batch = encodings.len();
        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0)
            .min(self.max_seq)
            .max(1);

        let mut input_ids = Array2::<i64>::zeros((batch, max_len));
        let mut attention_mask = Array2::<i64>::zeros((batch, max_len));
        let mut token_type_ids = Array2::<i64>::zeros((batch, max_len));
        for (i, encoding) in encodings.iter().enumerate() {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let types = encoding.get_type_ids();
            for j in 0..ids.len().min(max_len) {
                input_ids[[i, j]] = ids[j] as i64;
                attention_mask[[i, j]] = mask[j] as i64;
                token_type_ids[[i, j]] = types[j] as i64;
            }
        }

        let mut session = self
            .session
            .lock()
            .map_err(|e| anyhow::anyhow!("Reranker session lock poisoned: {e}"))?;
        let to_tensor = |a: Array2<i64>, name: &str| {
            Tensor::from_array(a)
                .map_err(|e| anyhow::anyhow!("Failed to create {name} tensor: {e}"))
        };
        let outputs = if self.wants_token_type_ids {
            session.run(ort::inputs![
                "input_ids" => to_tensor(input_ids, "input_ids")?,
                "attention_mask" => to_tensor(attention_mask, "attention_mask")?,
                "token_type_ids" => to_tensor(token_type_ids, "token_type_ids")?,
            ])
        } else {
            session.run(ort::inputs![
                "input_ids" => to_tensor(input_ids, "input_ids")?,
                "attention_mask" => to_tensor(attention_mask, "attention_mask")?,
            ])
        }
        .map_err(|e| anyhow::anyhow!("Reranker ONNX inference failed: {e}"))?;

        let (shape, data) = outputs[self.output_name.as_str()]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Failed to extract reranker logits: {e}"))?;

        // Accept [batch, 1] (the usual single-logit head) or flat [batch].
        let dims: Vec<usize> = shape.iter().map(|&d| d as usize).collect();
        let per_row = data.len() / batch;
        let single_logit = matches!(dims.as_slice(), [b, 1] | [b] if *b == batch);
        if !single_logit || per_row == 0 {
            anyhow::bail!(
                "reranker model output {:?} has shape {:?}; expected a single \
                 logit per pair ([batch, 1] or [batch]) — is this a cross-encoder?",
                self.output_name,
                dims
            );
        }

        Ok((0..batch).map(|i| data[i * per_row]).collect())
    }
}
