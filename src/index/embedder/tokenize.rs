//! Keep only model input tokens, not a batch of full transcript encodings.
use anyhow::{Context, Result};
use ndarray::Array2;
use tokenizers::{PaddingDirection, PaddingStrategy, Tokenizer};

struct Tokens {
    original_len: usize,
    ids: Vec<u32>,
    mask: Vec<u32>,
    types: Vec<u32>,
}

/// Tokenize one record at a time and release its offsets, strings and overflow
/// windows before the next record. Tokenizer truncation alone is not a memory
/// bound: it retains every overflow window, even though inference discards them.
/// Retained token storage is O(texts.len() * max_seq), plus ONE input's tokenizer
/// working set. This does not cap the size of an individual input or the corpus.
/// Preserve encode_batch's padding before applying the model's sequence limit.
pub(super) fn model_inputs(
    tokenizer: &Tokenizer,
    texts: &[&str],
    max_seq: usize,
) -> Result<(Array2<i64>, Array2<i64>, Array2<i64>)> {
    let mut rows = Vec::with_capacity(texts.len());
    for text in texts {
        let encoding = tokenizer
            .encode(*text, true)
            .map_err(|e| anyhow::anyhow!("Tokenization failed: {e}"))?;
        let len = encoding.len().min(max_seq);
        rows.push(Tokens {
            original_len: encoding.len(),
            ids: encoding.get_ids()[..len].to_vec(),
            mask: encoding.get_attention_mask()[..len].to_vec(),
            types: encoding.get_type_ids()[..len].to_vec(),
        });
        // encoding (including its overflow encodings) drops here, not after ONNX.
    }

    let longest = rows.iter().map(|row| row.original_len).max().unwrap_or(0);
    let padding = tokenizer.get_padding();
    let mut pad_to = padding.map_or(0, |p| match p.strategy {
        PaddingStrategy::Fixed(n) => n,
        PaddingStrategy::BatchLongest => longest,
    });
    if let Some(multiple) = padding
        .and_then(|p| p.pad_to_multiple_of)
        .filter(|n| *n > 0)
    {
        let remainder = pad_to % multiple;
        if remainder != 0 {
            pad_to = pad_to
                .checked_add(multiple - remainder)
                .context("padding length overflow")?;
        }
    }
    let width = longest.max(pad_to).min(max_seq);
    let mut ids = Array2::<i64>::zeros((rows.len(), width));
    let mut mask = Array2::<i64>::zeros((rows.len(), width));
    let mut types = Array2::<i64>::zeros((rows.len(), width));
    for (i, row) in rows.iter().enumerate() {
        let added = pad_to.saturating_sub(row.original_len);
        let left = padding.is_some_and(|p| matches!(p.direction, PaddingDirection::Left));
        let offset = if left { added } else { 0 };
        if let Some(p) = padding {
            let range = if left {
                0..added.min(width)
            } else {
                row.original_len.min(width)..pad_to.min(width)
            };
            for j in range {
                ids[[i, j]] = i64::from(p.pad_id);
                types[[i, j]] = i64::from(p.pad_type_id);
            }
        }
        for j in 0..row.ids.len().min(width.saturating_sub(offset)) {
            ids[[i, offset + j]] = i64::from(row.ids[j]);
            mask[[i, offset + j]] = i64::from(row.mask[j]);
            types[[i, offset + j]] = i64::from(row.types[j]);
        }
    }
    Ok((ids, mask, types))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokenizers::{
        models::wordlevel::WordLevel, pre_tokenizers::whitespace::Whitespace, PaddingParams,
        TruncationParams,
    };

    fn tokenizer() -> Tokenizer {
        let model = WordLevel::builder()
            .vocab(
                [
                    ("[UNK]".into(), 0),
                    ("hello".into(), 1),
                    ("world".into(), 2),
                ]
                .into_iter()
                .collect(),
            )
            .unk_token("[UNK]".into())
            .build()
            .unwrap();
        let mut t = Tokenizer::new(model);
        t.with_pre_tokenizer(Some(Whitespace));
        t
    }

    // Compare model inputs against the ORIGINAL batch tokenizer, including
    // padding modes that would be changed by naively replacing batch with encode.
    #[test]
    fn serial_model_inputs_match_batch_tokens_and_masks() {
        let long = "hello world 日本語 ".repeat(1000);
        let texts = ["", "hello", "hello world", long.as_str()];
        for strategy in [PaddingStrategy::BatchLongest, PaddingStrategy::Fixed(8)] {
            for direction in [PaddingDirection::Left, PaddingDirection::Right] {
                for truncate in [false, true] {
                    let mut t = tokenizer();
                    t.with_padding(Some(PaddingParams {
                        strategy: strategy.clone(),
                        direction,
                        pad_to_multiple_of: Some(4),
                        pad_id: 7,
                        pad_type_id: 9,
                        ..Default::default()
                    }));
                    if truncate {
                        t.with_truncation(Some(TruncationParams {
                            max_length: 7,
                            ..Default::default()
                        }))
                        .unwrap();
                    }
                    let original = t.encode_batch(texts.to_vec(), true).unwrap();
                    for max_seq in [2, 7, 16] {
                        let (ids, mask, types) = model_inputs(&t, &texts, max_seq).unwrap();
                        let width = original.iter().map(|e| e.len()).max().unwrap().min(max_seq);
                        assert_eq!(ids.dim(), (texts.len(), width));
                        for (i, row) in original.iter().enumerate() {
                            for j in 0..width {
                                assert_eq!(
                                    ids[[i, j]],
                                    i64::from(*row.get_ids().get(j).unwrap_or(&0))
                                );
                                assert_eq!(
                                    mask[[i, j]],
                                    i64::from(*row.get_attention_mask().get(j).unwrap_or(&0))
                                );
                                assert_eq!(
                                    types[[i, j]],
                                    i64::from(*row.get_type_ids().get(j).unwrap_or(&0))
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn no_padding_keeps_only_model_window() {
        let t = tokenizer();
        let long = "hello world ".repeat(10000);
        let (ids, mask, _) = model_inputs(&t, &["hello", &long], 8).unwrap();
        assert_eq!(ids.dim(), (2, 8));
        assert_eq!(mask.row(0).iter().sum::<i64>(), 1);
        assert_eq!(mask.row(1).iter().sum::<i64>(), 8);
        assert_eq!(model_inputs(&t, &[], 8).unwrap().0.dim(), (0, 0));
    }
}
