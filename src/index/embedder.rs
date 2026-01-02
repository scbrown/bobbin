use anyhow::{Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::Tokenizer;

const MODEL_NAME: &str = "all-MiniLM-L6-v2";
const EMBEDDING_DIM: usize = 384;
const MAX_SEQ_LENGTH: usize = 256;

/// HuggingFace model repository for ONNX sentence transformers
const HF_MODEL_REPO: &str = "sentence-transformers/all-MiniLM-L6-v2";

/// Generates embeddings using ONNX runtime with all-MiniLM-L6-v2
pub struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
}

impl Embedder {
    /// Load an embedding model from the cache directory
    pub fn load(cache_dir: &Path) -> Result<Self> {
        let model_dir = cache_dir.join(MODEL_NAME);
        let model_path = model_dir.join("model.onnx");
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() || !tokenizer_path.exists() {
            anyhow::bail!(
                "Model not found. Run `bobbin init` to download the model, or manually download to {}",
                model_dir.display()
            );
        }

        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {}", e))?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!("Failed to set thread count: {}", e))?
            .commit_from_file(&model_path)
            .map_err(|e| anyhow::anyhow!("Failed to load ONNX model from {}: {}", model_path.display(), e))?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        Ok(Self { session, tokenizer })
    }

    /// Generate embeddings for a batch of texts
    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let encodings = self
            .tokenizer
            .encode_batch(texts.to_vec(), true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {}", e))?;

        let batch_size = encodings.len();
        let max_len = encodings
            .iter()
            .map(|e| e.get_ids().len())
            .max()
            .unwrap_or(0)
            .min(MAX_SEQ_LENGTH);

        let mut input_ids = Array2::<i64>::zeros((batch_size, max_len));
        let mut attention_mask = Array2::<i64>::zeros((batch_size, max_len));
        let mut token_type_ids = Array2::<i64>::zeros((batch_size, max_len));

        for (i, encoding) in encodings.iter().enumerate() {
            let ids = encoding.get_ids();
            let mask = encoding.get_attention_mask();
            let type_ids = encoding.get_type_ids();

            let len = ids.len().min(max_len);
            for j in 0..len {
                input_ids[[i, j]] = ids[j] as i64;
                attention_mask[[i, j]] = mask[j] as i64;
                token_type_ids[[i, j]] = type_ids[j] as i64;
            }
        }

        // Keep a flat copy of attention mask for mean pooling
        let attention_mask_vec: Vec<i64> = attention_mask.iter().cloned().collect();

        let input_ids_tensor = Tensor::from_array(input_ids)
            .map_err(|e| anyhow::anyhow!("Failed to create input_ids tensor: {}", e))?;
        let attention_mask_tensor = Tensor::from_array(attention_mask)
            .map_err(|e| anyhow::anyhow!("Failed to create attention_mask tensor: {}", e))?;
        let token_type_ids_tensor = Tensor::from_array(token_type_ids)
            .map_err(|e| anyhow::anyhow!("Failed to create token_type_ids tensor: {}", e))?;

        let outputs = self
            .session
            .run(ort::inputs![
                "input_ids" => input_ids_tensor,
                "attention_mask" => attention_mask_tensor,
                "token_type_ids" => token_type_ids_tensor,
            ])
            .map_err(|e| anyhow::anyhow!("ONNX inference failed: {}", e))?;

        // Get the last hidden state (batch_size, seq_len, hidden_dim)
        let (shape, data) = outputs["last_hidden_state"]
            .try_extract_tensor::<f32>()
            .map_err(|e| anyhow::anyhow!("Failed to extract output tensor: {}", e))?;

        let shape_dims: Vec<usize> = shape.iter().map(|&x| x as usize).collect();
        let seq_len = shape_dims[1];
        let hidden_dim = shape_dims[2];

        // Mean pooling with attention mask
        let mut embeddings = Vec::with_capacity(batch_size);

        for i in 0..batch_size {
            let mut sum = vec![0.0f32; hidden_dim];
            let mut count = 0.0f32;

            for j in 0..max_len.min(seq_len) {
                if attention_mask_vec[i * max_len + j] == 1 {
                    let offset = (i * seq_len + j) * hidden_dim;
                    for k in 0..hidden_dim {
                        sum[k] += data[offset + k];
                    }
                    count += 1.0;
                }
            }

            // Average and normalize
            if count > 0.0 {
                for v in &mut sum {
                    *v /= count;
                }
            }

            // L2 normalize
            let norm: f32 = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm > 0.0 {
                for v in &mut sum {
                    *v /= norm;
                }
            }

            embeddings.push(sum);
        }

        Ok(embeddings)
    }

    /// Generate embedding for a single text
    pub fn embed(&mut self, text: &str) -> Result<Vec<f32>> {
        self.embed_batch(&[text])
            .map(|v| v.into_iter().next().unwrap())
    }

    /// Get the embedding dimension
    pub fn dimension(&self) -> usize {
        EMBEDDING_DIM
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        MODEL_NAME
    }
}

/// Thread-safe wrapper for the embedder
pub type SharedEmbedder = Arc<Embedder>;

/// Download the embedding model if not present
pub async fn ensure_model(cache_dir: &Path) -> Result<PathBuf> {
    let model_dir = cache_dir.join(MODEL_NAME);
    let model_path = model_dir.join("model.onnx");
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        return Ok(model_dir);
    }

    std::fs::create_dir_all(&model_dir)
        .with_context(|| format!("Failed to create model directory: {}", model_dir.display()))?;

    eprintln!("Downloading embedding model {}...", MODEL_NAME);

    // Download model.onnx from HuggingFace
    let model_url = format!(
        "https://huggingface.co/{}/resolve/main/onnx/model.onnx",
        HF_MODEL_REPO
    );
    download_file(&model_url, &model_path).await?;

    // Download tokenizer.json
    let tokenizer_url = format!(
        "https://huggingface.co/{}/resolve/main/tokenizer.json",
        HF_MODEL_REPO
    );
    download_file(&tokenizer_url, &tokenizer_path).await?;

    eprintln!("Model downloaded successfully to {}", model_dir.display());
    Ok(model_dir)
}

/// Download a file from a URL to a local path
async fn download_file(url: &str, path: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("Failed to download {}", url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "Failed to download {}: HTTP {}",
            url,
            response.status()
        );
    }

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read response from {}", url))?;

    let mut file = tokio::fs::File::create(path)
        .await
        .with_context(|| format!("Failed to create file {}", path.display()))?;

    file.write_all(&bytes)
        .await
        .with_context(|| format!("Failed to write to {}", path.display()))?;

    Ok(())
}
