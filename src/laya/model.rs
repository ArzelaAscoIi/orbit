//! Laya in-process: the checkpoint from Hugging Face, pinned to the revision
//! it was evaluated at, downloaded once into `<data>/laya/<revision>/`, and
//! run on the CPU with candle. No Python, no server.
//!
//! Laya is ModernBERT-large (candle has it) plus a small decision head,
//! ported here from laya 0.3.20 (`laya/common.py` `build_sequence` and
//! `DecisionModel`): two pre-norm transformer layers, a type embedding, and
//! a scorer read at one `[MASK]` per option. tests/laya-parity.json holds
//! Python's ids and answers for the same notes; the parity test keeps this
//! port honest.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use candle_core::{D, DType, Device, IndexOp, Module, Tensor};
use candle_nn::{LayerNorm, Linear, VarBuilder};
use candle_transformers::models::modernbert;

/// The English checkpoint at the revision the evaluation used.
const REPO: &str = "convaiinnovations/laya";
pub const REVISION: &str = "55cf4c4ebb4ebe31b2550e8bdf3bd21b99753851";
/// `(path in the repo, bytes)`: what it needs, and how big, for progress
/// and to tell a finished download from a cut one.
pub const FILES: [(&str, u64); 4] = [
    ("rl_agent_config.json", 745),
    ("encoder/config.json", 2_083),
    ("tokenizer/tokenizer.json", 3_583_228),
    ("model.safetensors", 842_609_210),
];

const MAX_LEN: usize = 512;
const HEAD_MAX_LEN: usize = 192;

pub fn dir() -> PathBuf {
    crate::store::data_dir().join("laya").join(REVISION)
}

/// Every file is there (a download writes `.part` files, renamed at the end).
pub fn downloaded(dir: &Path) -> bool {
    FILES.iter().all(|(file, _)| dir.join(file).is_file())
}

/// Fetches whatever is missing, reporting `(done, total)` bytes.
pub async fn download(
    http: &reqwest::Client,
    dir: &Path,
    progress: impl Fn(u64, u64),
) -> Result<()> {
    let total: u64 = FILES.iter().map(|(_, bytes)| bytes).sum();
    let mut done = 0;
    for (file, _) in FILES {
        let path = dir.join(file);
        if path.is_file() {
            done += std::fs::metadata(&path).map_or(0, |m| m.len());
            continue;
        }
        std::fs::create_dir_all(path.parent().context("no folder")?)?;
        let url = format!("https://huggingface.co/{REPO}/resolve/{REVISION}/{file}");
        let mut response = http
            .get(&url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .with_context(|| format!("cannot download {file}"))?;
        let part = path.with_extension("part");
        let mut out = std::fs::File::create(&part)?;
        while let Some(chunk) = response.chunk().await? {
            std::io::Write::write_all(&mut out, &chunk)?;
            done += chunk.len() as u64;
            progress(done, total.max(done));
        }
        std::fs::rename(&part, &path)?;
    }
    Ok(())
}

/// One noul (yes or no) question's answer: `p` that it holds, and the
/// calibrated confidence (the larger of `p` and `1 - p`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Noul {
    pub p: f64,
    pub confidence: f64,
}

pub struct Laya {
    tokenizer: tokenizers::Tokenizer,
    encoder: modernbert::ModernBert,
    head: Vec<HeadLayer>,
    /// `type_emb` row 2: noul.
    noul: Tensor,
    scorer: (LayerNorm, Linear, Linear),
    /// `temperature_by_options["noul:2"]`, clamped as laya does.
    temperature: f64,
    special: Special,
    device: Device,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Special {
    cls: u32,
    sep: u32,
    mask: u32,
}

impl Laya {
    pub fn load(dir: &Path) -> Result<Self> {
        let device = Device::Cpu;
        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer/tokenizer.json"))
            .map_err(|e| anyhow!("Laya's tokenizer: {e}"))?;
        let special = special(&tokenizer)?;
        let config = encoder_config(&std::fs::read_to_string(dir.join("encoder/config.json"))?)?;
        let agent: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("rl_agent_config.json"))?)?;
        let temperature = agent["temperature_by_options"]["noul:2"]
            .as_f64()
            .or(agent["temperature"][2].as_f64())
            .unwrap_or(1.0)
            .clamp(0.5, 5.0);
        // SAFETY: the file is ours, written once by `download` and never
        // changed while mapped.
        let vb = unsafe {
            VarBuilder::from_mmaped_safetensors(
                &[dir.join("model.safetensors")],
                DType::F32,
                &device,
            )?
        };
        // candle names the backbone `model.*`; Laya's checkpoint `encoder.*`.
        let encoder = modernbert::ModernBert::load(
            vb.clone()
                .rename_f(|name| name.replacen("model.", "encoder.", 1)),
            &config,
        )?;
        let d = config.hidden_size;
        let head = (0..2)
            .map(|i| HeadLayer::load(vb.pp(format!("head.layers.{i}")), d))
            .collect::<Result<_>>()?;
        let noul = vb.get((3, d), "type_emb.weight")?.i(2)?;
        let scorer = (
            candle_nn::layer_norm(d, 1e-5, vb.pp("scorer.0"))?,
            candle_nn::linear(d, d, vb.pp("scorer.1"))?,
            candle_nn::linear(d, 1, vb.pp("scorer.3"))?,
        );
        Ok(Self {
            tokenizer,
            encoder,
            head,
            noul,
            scorer,
            temperature,
            special,
            device,
        })
    }

    /// The ids and `[MASK]` positions laya's `build_sequence` makes for a
    /// noul question about `state`.
    pub fn sequence(&self, state: &str, instructions: &str) -> Result<(Vec<u32>, Vec<usize>)> {
        let encode = |text: &str| -> Result<Vec<u32>> {
            Ok(self
                .tokenizer
                .encode(text, false)
                .map_err(|e| anyhow!("{e}"))?
                .get_ids()
                .to_vec())
        };
        sequence(&encode, self.special, state, instructions)
    }

    /// Answers one yes-or-no question about `state`. CPU-heavy: run it off
    /// the async runtime (`spawn_blocking`).
    pub fn noul(&self, state: &str, instructions: &str) -> Result<Noul> {
        let (ids, markers) = self.sequence(state, instructions)?;
        if markers.len() != 2 {
            bail!("the question does not fit");
        }
        let n = ids.len();
        let input = Tensor::new(ids.as_slice(), &self.device)?.unsqueeze(0)?;
        let mask = Tensor::ones((1, n), DType::U32, &self.device)?;
        let mut h = self.encoder.forward(&input, &mask)?;
        h = h.broadcast_add(&self.noul)?;
        for layer in &self.head {
            h = layer.forward(&h)?;
        }
        let at = Tensor::new(
            markers
                .iter()
                .map(|&m| m as u32)
                .collect::<Vec<_>>()
                .as_slice(),
            &self.device,
        )?;
        let m = h.i(0)?.index_select(&at, 0)?;
        let (norm, dense, out) = &self.scorer;
        let logits = out
            .forward(&dense.forward(&norm.forward(&m)?)?.gelu_erf()?)?
            .squeeze(D::Minus1)?
            .to_vec1::<f32>()?;
        Ok(calibrate(&logits, self.temperature))
    }
}

/// `softmax(logits / t)`: `[false, true]` for a noul question.
fn calibrate(logits: &[f32], t: f64) -> Noul {
    let z: Vec<f64> = logits.iter().map(|&l| f64::from(l) / t).collect();
    let max = z.iter().copied().fold(f64::MIN, f64::max);
    let e: Vec<f64> = z.iter().map(|v| (v - max).exp()).collect();
    let sum: f64 = e.iter().sum();
    let p = e[1] / sum;
    Noul {
        p,
        confidence: p.max(1.0 - p),
    }
}

/// laya's `build_sequence` for a noul question with the default labels:
/// `[CLS] noul question: <ins> [SEP] [MASK] false: ... [MASK] true: ... [SEP] state [SEP]`.
fn sequence(
    encode: &dyn Fn(&str) -> Result<Vec<u32>>,
    special: Special,
    state: &str,
    instructions: &str,
) -> Result<(Vec<u32>, Vec<usize>)> {
    const OPTIONS: [&str; 2] = [
        "false: no, the statement does not hold",
        "true: yes, the statement holds",
    ];
    let clean = |text: &str| text.replace("[MASK]", " ");
    let mut head = encode(&format!("noul question: {}", clean(instructions)))?;
    let options: Vec<Vec<u32>> = OPTIONS
        .iter()
        .map(|o| {
            let mut ids = vec![special.mask];
            ids.extend(encode(&format!(" {o}"))?.into_iter().take(48));
            Ok(ids)
        })
        .collect::<Result<_>>()?;
    let budget = HEAD_MAX_LEN.saturating_sub(options.iter().map(Vec::len).sum());
    head.truncate(budget.max(8));
    let mut ids = vec![special.cls];
    ids.extend(head);
    ids.push(special.sep);
    let mut markers = Vec::new();
    for option in options {
        markers.push(ids.len());
        ids.extend(option);
    }
    ids.push(special.sep);
    let room = MAX_LEN.saturating_sub(ids.len() + 1);
    ids.extend(encode(&clean(state))?.into_iter().take(room));
    ids.push(special.sep);
    ids.truncate(MAX_LEN);
    markers.retain(|&m| m < MAX_LEN);
    Ok((ids, markers))
}

fn special(tokenizer: &tokenizers::Tokenizer) -> Result<Special> {
    let id = |token: &str| {
        tokenizer
            .token_to_id(token)
            .ok_or_else(|| anyhow!("Laya's tokenizer has no {token}"))
    };
    Ok(Special {
        cls: id("[CLS]")?,
        sep: id("[SEP]")?,
        mask: id("[MASK]")?,
    })
}

/// candle's ModernBERT config from the checkpoint's: newer transformers
/// keep the rope thetas under `rope_parameters`.
fn encoder_config(json: &str) -> Result<modernbert::Config> {
    let mut c: serde_json::Value = serde_json::from_str(json)?;
    let theta = |kind: &str, fallback: f64| {
        c["rope_parameters"][kind]["rope_theta"]
            .as_f64()
            .unwrap_or(fallback)
    };
    let (global, local) = (
        c["global_rope_theta"]
            .as_f64()
            .unwrap_or(theta("full_attention", 160_000.0)),
        c["local_rope_theta"]
            .as_f64()
            .unwrap_or(theta("sliding_attention", 10_000.0)),
    );
    c["global_rope_theta"] = global.into();
    c["local_rope_theta"] = local.into();
    Ok(serde_json::from_value(c)?)
}

/// PyTorch's `nn.TransformerEncoderLayer(d, d / 64 heads, 4d, norm_first,
/// relu)`, as laya's head uses it, at inference.
struct HeadLayer {
    norm1: LayerNorm,
    norm2: LayerNorm,
    in_proj: Linear,
    out_proj: Linear,
    linear1: Linear,
    linear2: Linear,
    heads: usize,
}

impl HeadLayer {
    fn load(vb: VarBuilder, d: usize) -> Result<Self> {
        let attn = vb.pp("self_attn");
        let in_proj = Linear::new(
            attn.get((3 * d, d), "in_proj_weight")?,
            Some(attn.get(3 * d, "in_proj_bias")?),
        );
        Ok(Self {
            norm1: candle_nn::layer_norm(d, 1e-5, vb.pp("norm1"))?,
            norm2: candle_nn::layer_norm(d, 1e-5, vb.pp("norm2"))?,
            in_proj,
            out_proj: candle_nn::linear(d, d, attn.pp("out_proj"))?,
            linear1: candle_nn::linear(d, 4 * d, vb.pp("linear1"))?,
            linear2: candle_nn::linear(4 * d, d, vb.pp("linear2"))?,
            heads: (d / 64).max(1),
        })
    }

    /// One sequence, no padding, so no key mask.
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let (b, n, d) = x.dims3()?;
        let hd = d / self.heads;
        let qkv = self.in_proj.forward(&self.norm1.forward(x)?)?;
        let split = |i: usize| -> Result<Tensor> {
            Ok(qkv
                .narrow(D::Minus1, i * d, d)?
                .reshape((b, n, self.heads, hd))?
                .transpose(1, 2)?
                .contiguous()?)
        };
        let (q, k, v) = (split(0)?, split(1)?, split(2)?);
        let scores = (q.matmul(&k.transpose(2, 3)?.contiguous()?)? / (hd as f64).sqrt())?;
        let attn = candle_nn::ops::softmax_last_dim(&scores)?.matmul(&v)?;
        let attn = attn.transpose(1, 2)?.reshape((b, n, d))?;
        let x = (x + self.out_proj.forward(&attn)?)?;
        let ff = self
            .linear2
            .forward(&self.linear1.forward(&self.norm2.forward(&x)?)?.relu()?)?;
        Ok((x + ff)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temperature_softens_and_confidence_is_the_larger_side() {
        let sharp = calibrate(&[0.0, 4.0], 1.0);
        let soft = calibrate(&[0.0, 4.0], 2.0);
        assert!(sharp.p > soft.p && soft.p > 0.5);
        assert_eq!(soft.confidence, soft.p);
        let no = calibrate(&[3.0, 0.0], 2.0);
        assert!(no.p < 0.5 && (no.confidence - (1.0 - no.p)).abs() < 1e-12);
    }

    #[test]
    fn the_sequence_has_laya_s_shape_and_fits_512() {
        // One id per word keeps the layout readable.
        let encode = |text: &str| -> Result<Vec<u32>> {
            Ok(text
                .split_whitespace()
                .map(|w| w.len() as u32 + 100)
                .collect())
        };
        let special = Special {
            cls: 1,
            sep: 2,
            mask: 3,
        };
        let (ids, markers) =
            sequence(&encode, special, "it fails", "Is it [MASK] broken?").unwrap();
        assert_eq!(ids[0], 1);
        assert_eq!(markers.len(), 2);
        assert!(markers.iter().all(|&m| ids[m] == 3));
        assert_eq!(
            ids.iter().filter(|&&i| i == 3).count(),
            2,
            "the [MASK] in the question is gone"
        );
        assert_eq!(*ids.last().unwrap(), 2);

        let long = "word ".repeat(2000);
        let (ids, markers) = sequence(&encode, special, &long, "Is it broken?").unwrap();
        assert_eq!(ids.len(), MAX_LEN);
        assert_eq!(
            *ids.last().unwrap(),
            2,
            "a long state is cut, the closing [SEP] kept"
        );
        assert_eq!(markers.len(), 2);
    }

    #[test]
    fn rope_thetas_come_from_rope_parameters() {
        let c = encoder_config(
            r#"{"vocab_size":10,"hidden_size":64,"num_hidden_layers":1,"num_attention_heads":1,
                "intermediate_size":8,"max_position_embeddings":16,"layer_norm_eps":1e-5,
                "pad_token_id":0,"global_attn_every_n_layers":3,"local_attention":128,
                "rope_parameters":{"full_attention":{"rope_theta":160000.0},
                                   "sliding_attention":{"rope_theta":10000.0}}}"#,
        )
        .unwrap();
        assert_eq!(
            (c.global_rope_theta, c.local_rope_theta),
            (160_000.0, 10_000.0)
        );
    }

    /// The real download from Hugging Face into a fresh folder, then one
    /// answer from what arrived. 840 MB: `cargo test --release download -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn download_from_hugging_face() {
        let dir = std::env::temp_dir().join(format!("laya-download-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let seen = std::sync::atomic::AtomicU64::new(0);
        download(&crate::http(), &dir, |done, total| {
            assert!(done <= total);
            seen.store(done, std::sync::atomic::Ordering::Relaxed);
        })
        .await
        .unwrap();
        assert!(downloaded(&dir));
        for (file, bytes) in FILES {
            assert_eq!(
                std::fs::metadata(dir.join(file)).unwrap().len(),
                bytes,
                "{file}"
            );
        }
        assert_eq!(seen.into_inner(), FILES.iter().map(|(_, b)| b).sum::<u64>());
        let laya = Laya::load(&dir).unwrap();
        assert!(
            laya.noul("Login fails with a 500 error", "Is something broken?")
                .unwrap()
                .p
                > 0.5
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The port against Python's laya on the same notes: the same ids, and
    /// the same answer to 1e-3. Needs the checkpoint (Settings downloads it):
    /// `cargo test --release parity -- --ignored`
    #[test]
    #[ignore]
    fn parity_with_python_laya() {
        let laya = Laya::load(&dir()).expect("download Laya in Settings first");
        let parity: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/laya-parity.json")).unwrap();
        assert_eq!(parity["revision"], REVISION);
        let question = parity["question"].as_str().unwrap();
        assert_eq!(
            question,
            super::super::BUG
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        );
        for case in parity["cases"].as_array().unwrap() {
            let note = case["note"].as_str().unwrap();
            let (ids, markers) = laya.sequence(note, question).unwrap();
            let want: Vec<u32> = serde_json::from_value(case["ids"].clone()).unwrap();
            assert_eq!(ids, want, "ids for {note}");
            assert_eq!(
                markers,
                serde_json::from_value::<Vec<usize>>(case["markers"].clone()).unwrap()
            );
            let got = laya.noul(note, question).unwrap();
            let p = case["noul"].as_f64().unwrap();
            assert!(
                (got.p - p).abs() < 1e-3,
                "{note}: rust {} python {p}",
                got.p
            );
        }
    }
}
