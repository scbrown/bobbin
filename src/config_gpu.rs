//! GPU resolution for the embedding backend (sidecar of `config.rs`,
//! split under the file-size ratchet — same struct, same behavior).

use super::{EmbeddingBackend, EmbeddingConfig};

impl EmbeddingConfig {
    /// Check if GPU should be used.
    ///
    /// Resolution order:
    /// 1. `BOBBIN_GPU=0` / `false` → force CPU
    /// 2. `BOBBIN_GPU=1` / `true` → force GPU
    /// 3. Config `gpu = true` → force GPU
    /// 4. Otherwise → auto-detect CUDA availability
    pub fn use_gpu(&self) -> bool {
        if let Ok(v) = std::env::var("BOBBIN_GPU") {
            if v == "0" || v.eq_ignore_ascii_case("false") {
                return false;
            }
            if v == "1" || v.eq_ignore_ascii_case("true") {
                return true;
            }
        }
        if self.gpu {
            return true;
        }
        // Auto-detect: only for ONNX backend
        if self.backend == EmbeddingBackend::Onnx {
            return Self::detect_cuda();
        }
        false
    }

    /// Probe whether the CUDA execution provider is available at runtime.
    fn detect_cuda() -> bool {
        crate::index::embedder::auto_resolve_gpu_dylib();
        use ort::ep::{ExecutionProvider, CUDA};
        CUDA::default().is_available().unwrap_or(false)
    }
}
