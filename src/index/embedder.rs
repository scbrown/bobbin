use anyhow::{Context, Result};
use ndarray::Array2;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::Tokenizer;

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub name: String,
    pub repo: String,
    pub dim: usize,
    pub max_seq: usize,
    pub onnx_path: String,
}

impl ModelConfig {
    pub fn get(name: &str) -> Result<Self> {
        match name {
            "all-MiniLM-L6-v2" => Ok(Self {
                name: name.to_string(),
                repo: "Xenova/all-MiniLM-L6-v2".to_string(),
                dim: 384,
                max_seq: 256,
                onnx_path: "onnx/model.onnx".to_string(),
            }),
            "bge-small-en-v1.5" => Ok(Self {
                name: name.to_string(),
                repo: "Xenova/bge-small-en-v1.5".to_string(),
                dim: 384,
                max_seq: 512,
                onnx_path: "onnx/model.onnx".to_string(),
            }),
            "gte-small" => Ok(Self {
                name: name.to_string(),
                repo: "Xenova/gte-small".to_string(),
                dim: 384,
                max_seq: 512,
                onnx_path: "onnx/model.onnx".to_string(),
            }),
            _ => anyhow::bail!(
                "Unsupported model: {}. Supported: all-MiniLM-L6-v2, bge-small-en-v1.5, gte-small",
                name
            ),
        }
    }
}

/// Generates embeddings using ONNX runtime
pub struct Embedder {
    session: Session,
    tokenizer: Tokenizer,
    config: ModelConfig,
}

impl Embedder {
    /// Load an embedding model from the cache directory
    pub fn load(cache_dir: &Path, model_name: &str) -> Result<Self> {
        let config = ModelConfig::get(model_name)?;
        let model_dir = cache_dir.join(&config.name);
        let model_path = model_dir.join(&config.onnx_path);
        let tokenizer_path = model_dir.join("tokenizer.json");

        if !model_path.exists() || !tokenizer_path.exists() {
            anyhow::bail!(
                "Model {} not found. Run `bobbin init` or `bobbin index` to download it.",
                model_name
            );
        }

        let session = Session::builder()
            .map_err(|e| anyhow::anyhow!("Failed to create ONNX session builder: {}", e))?
            .with_intra_threads(4)
            .map_err(|e| anyhow::anyhow!("Failed to set thread count: {}", e))?
            .commit_from_file(&model_path)
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to load ONNX model from {}: {}",
                    model_path.display(),
                    e
                )
            })?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| anyhow::anyhow!("Failed to load tokenizer: {}", e))?;

        Ok(Self {
            session,
            tokenizer,
            config,
        })
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
            .min(self.config.max_seq);

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
        self.config.dim
    }

    /// Get the model name
    pub fn model_name(&self) -> &str {
        &self.config.name
    }
}

/// Thread-safe wrapper for the embedder
pub type SharedEmbedder = Arc<Embedder>;

/// Download the embedding model if not present
pub async fn ensure_model(cache_dir: &Path, model_name: &str) -> Result<PathBuf> {
    let config = ModelConfig::get(model_name)?;
    let model_dir = cache_dir.join(&config.name);
    let model_path = model_dir.join(&config.onnx_path);
    let tokenizer_path = model_dir.join("tokenizer.json");

    if model_path.exists() && tokenizer_path.exists() {
        return Ok(model_dir);
    }

    std::fs::create_dir_all(&model_dir)
        .with_context(|| format!("Failed to create model directory: {}", model_dir.display()))?;

    if let Some(parent) = model_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "Failed to create model parent directory: {}",
                    parent.display()
                )
            })?;
        }
    }

    eprintln!("Downloading embedding model {}...", model_name);

    // Download model.onnx from HuggingFace
    let model_url = format!(
        "https://huggingface.co/{}/resolve/main/{}",
        config.repo, config.onnx_path
    );
    download_file(&model_url, &model_path).await?;

    // Download tokenizer.json
    let tokenizer_url = format!(
        "https://huggingface.co/{}/resolve/main/tokenizer.json",
        config.repo
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
        anyhow::bail!("Failed to download {}: HTTP {}", url, response.status());
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
