use super::bs1770;
use crate::TtsError;
use candle_core::utils::{cuda_is_available, metal_is_available};
use candle_core::{Device, Tensor};
use candle_transformers::models::metavoice::{tokenizers, transformer};
use candle_transformers::models::quantized_metavoice::transformer as qtransformer;
use std::error::Error;
use std::io::Write;
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Transformer {
    Normal(transformer::Model),
    Quantized(qtransformer::Model),
}

pub fn get_fs_tokenizer(
    first_stage_meta: serde_json::Value,
) -> Result<tokenizers::BPE, Box<dyn Error>> {
    let first_stage_tokenizer = match first_stage_meta.as_object() {
        None => return Err(TtsError::Json.into()),
        Some(j) => match j.get("tokenizer") {
            None => return Err(TtsError::NoTokenizerKey.into()),
            Some(j) => j,
        },
    };

    Ok(tokenizers::BPE::from_json(first_stage_tokenizer, 512)?)
}
pub fn device(cpu: bool) -> Result<Device, Box<dyn Error>> {
    if cpu {
        Ok(Device::Cpu)
    } else if cuda_is_available() {
        Ok(Device::new_cuda(0)?)
    } else if metal_is_available() {
        Ok(Device::new_metal(0)?)
    } else {
        Ok(Device::Cpu)
    }
}

pub fn hub_load_safetensors(
    repo: &hf_hub::api::sync::ApiRepo,
    json_file: &str,
) -> Result<Vec<std::path::PathBuf>, Box<dyn Error>> {
    let json_file = repo.get(json_file).unwrap();
    let json_file = std::fs::File::open(json_file)?;
    let json: serde_json::Value = serde_json::from_reader(&json_file).unwrap();
    let weight_map = match json.get("weight_map") {
        None => return Err("no weight map in {json_file:?}".into()),
        Some(serde_json::Value::Object(map)) => map,
        Some(_) => return Err("weight map in {json_file:?} is not a map".into()),
    };
    let mut safetensors_files = std::collections::HashSet::new();
    for value in weight_map.values() {
        if let Some(file) = value.as_str() {
            safetensors_files.insert(file.to_string());
        }
    }
    let safetensors_files = safetensors_files
        .iter()
        .map(|v| repo.get(v).unwrap())
        .collect::<Vec<PathBuf>>();
    Ok(safetensors_files)
}

pub trait Sample {
    fn to_i16(&self) -> i16;
}

impl Sample for f32 {
    fn to_i16(&self) -> i16 {
        (self.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}

impl Sample for f64 {
    fn to_i16(&self) -> i16 {
        (self.clamp(-1.0, 1.0) * 32767.0) as i16
    }
}

impl Sample for i16 {
    fn to_i16(&self) -> i16 {
        *self
    }
}

pub fn write_pcm_as_wav<W: Write, S: Sample>(
    w: &mut W,
    samples: &[S],
    sample_rate: u32,
) -> std::io::Result<()> {
    let len = 12u32; // header
    let len = len + 24u32; // fmt
    let len = len + samples.len() as u32 * 2 + 8; // data
    let n_channels = 1u16;
    let bytes_per_second = sample_rate * 2 * n_channels as u32;
    w.write_all(b"RIFF")?;
    w.write_all(&(len - 8).to_le_bytes())?; // total length minus 8 bytes
    w.write_all(b"WAVE")?;

    // Format block
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?; // block len minus 8 bytes
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&n_channels.to_le_bytes())?; // one channel
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&bytes_per_second.to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?; // 2 bytes of data per sample
    w.write_all(&16u16.to_le_bytes())?; // bits per sample

    // Data block
    w.write_all(b"data")?;
    w.write_all(&(samples.len() as u32 * 2).to_le_bytes())?;
    for sample in samples.iter() {
        w.write_all(&sample.to_i16().to_le_bytes())?
    }
    Ok(())
}

pub fn normalize_loudness(
    wav: &Tensor,
    sample_rate: u32,
    loudness_compressor: bool,
) -> Result<candle_core::Tensor, Box<dyn Error>> {
    let energy = wav.sqr()?.mean_all()?.sqrt()?.to_vec0::<f32>()?;
    if energy < 2e-3 {
        return Ok(wav.clone());
    }
    let wav_array = wav.to_vec1::<f32>()?;
    let mut meter = bs1770::ChannelLoudnessMeter::new(sample_rate);
    meter.push(wav_array.into_iter());
    let power = meter.as_100ms_windows();
    let loudness = match bs1770::gated_mean(power) {
        None => return Ok(wav.clone()),
        Some(gp) => gp.loudness_lkfs() as f64,
    };
    let delta_loudness = -14. - loudness;
    let gain = 10f64.powf(delta_loudness / 20.);
    let wav = (wav * gain)?;
    if loudness_compressor {
        Ok(wav.tanh()?)
    } else {
        Ok(wav)
    }
}
