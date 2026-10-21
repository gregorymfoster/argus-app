//! Core ML on macOS (ADR 0015): the GPU and Neural Engine path for a model too heavy for
//! the CPU, AI Denoise only for now. tract (`analyzers::runtime`) stays the reference every
//! test checks against and the fallback when Core ML can't load or run.
//!
//! A model ships as an `.mlpackage` embedded in the binary. On first use it is written to
//! the temp folder, compiled for this Mac (a few seconds) and kept there under a hash of its
//! bytes, so later launches load the compiled model straight away and a new model never
//! meets an old compile. `ARGUS_COREML_UNITS` (all, gpu, ane, cpu) picks the hardware for
//! comparisons; the default lets Core ML choose.

use crate::{Error, Result};
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::AllocAnyThread;
use objc2_core_ml::{
    MLArrayBatchProvider, MLBatchProvider, MLComputeUnits, MLDictionaryFeatureProvider,
    MLFeatureProvider, MLFeatureValue, MLModel, MLModelConfiguration, MLMultiArray,
    MLMultiArrayDataType,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// An embedded `.mlpackage` (its files, by path inside the package) with one fixed-shape
/// float input and one float output, loaded on first use.
pub struct Lazy {
    files: &'static [(&'static str, &'static [u8])],
    shape: &'static [usize],
    input: &'static str,
    output: &'static str,
    model: OnceLock<std::result::Result<Model, String>>,
}

impl Lazy {
    pub const fn new(
        files: &'static [(&'static str, &'static [u8])],
        shape: &'static [usize],
        input: &'static str,
        output: &'static str,
    ) -> Self {
        Self {
            files,
            shape,
            input,
            output,
            model: OnceLock::new(),
        }
    }

    pub fn get(&self) -> Result<&Model> {
        self.model
            .get_or_init(|| {
                let started = std::time::Instant::now();
                let m = load(self.files, self.shape, self.input, self.output).map_err(|e| e.0);
                crate::debug_log::event(
                    "coreml_load",
                    serde_json::json!({
                        "ok": m.is_ok(),
                        "error": m.as_ref().err(),
                        "units": format!("{:?}", units()),
                        "ms": started.elapsed().as_millis() as u64,
                    }),
                );
                m
            })
            .as_ref()
            .map_err(|e| Error(e.clone()))
    }
}

pub struct Model {
    model: Retained<MLModel>,
    shape: Vec<usize>,
    input: Retained<NSString>,
    output: Retained<NSString>,
    /// One batch at a time: Core ML spreads each over the GPU and Neural Engine.
    busy: Mutex<()>,
    /// Set when a batch never came back: Core ML is not used again in this process.
    stuck: AtomicBool,
}

// SAFETY: the Objective-C objects are only touched through `run`, one call at a time
// (`busy`), and MLModel itself may be used from any thread.
unsafe impl Send for Model {}
unsafe impl Sync for Model {}

/// How long one batch may take before Core ML is given up on. A batch of tiles takes about
/// a second on an M4 Max; the first one also warms the GPU up.
const STALL: Duration = Duration::from_secs(120);

impl Model {
    /// Run a batch of inputs, each laid out first-major (row-major) at the model's shape,
    /// in one Core ML call (`predictionsFromBatch`, which keeps the GPU fed between them);
    /// returns the outputs the same way. A batch that hasn't come back after [`STALL`]
    /// (a Metal hang was seen once, 2026-10-21) fails, and so does every later call, so
    /// the caller falls back to the CPU instead of waiting forever.
    pub fn predict(&'static self, inputs: Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>> {
        if self.stuck.load(Ordering::Relaxed) {
            return Err(Error("Core ML stalled earlier in this run".into()));
        }
        let n: usize = self.shape.iter().product();
        if let Some(bad) = inputs.iter().find(|i| i.len() != n) {
            return Err(Error(format!(
                "Core ML: {} values for shape {:?}",
                bad.len(),
                self.shape
            )));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("argus-coreml".into())
            .spawn(move || {
                let _ = tx.send(self.run(&inputs));
            })
            .map_err(|e| Error(format!("Core ML: {e}")))?;
        match rx.recv_timeout(STALL) {
            Ok(out) => out,
            Err(_) => {
                self.stuck.store(true, Ordering::Relaxed);
                Err(Error(format!(
                    "Core ML: no answer after {} s",
                    STALL.as_secs()
                )))
            }
        }
    }

    /// False once a batch has stalled.
    pub fn usable(&self) -> bool {
        !self.stuck.load(Ordering::Relaxed)
    }

    fn run(&self, inputs: &[Vec<f32>]) -> Result<Vec<Vec<f32>>> {
        let _busy = self.busy.lock().unwrap_or_else(|e| e.into_inner());
        autoreleasepool(|_| unsafe {
            let providers = inputs
                .iter()
                .map(|input| self.provider(input))
                .collect::<Result<Vec<_>>>()?;
            let providers: Vec<&ProtocolObject<dyn MLFeatureProvider>> = providers
                .iter()
                .map(|p| ProtocolObject::from_ref(&**p))
                .collect();
            let batch = MLArrayBatchProvider::initWithFeatureProviderArray(
                MLArrayBatchProvider::alloc(),
                &NSArray::from_slice(&providers),
            );
            let out = self
                .model
                .predictionsFromBatch_error(ProtocolObject::from_ref(&*batch))
                .map_err(ns_error)?;
            (0..out.count())
                .map(|i| {
                    let array = out
                        .featuresAtIndex(i)
                        .featureValueForName(&self.output)
                        .and_then(|v| v.multiArrayValue())
                        .ok_or_else(|| Error("Core ML: no output array".into()))?;
                    read(&array)
                })
                .collect()
        })
    }

    /// One input as a feature provider holding a new float array.
    unsafe fn provider(&self, input: &[f32]) -> Result<Retained<MLDictionaryFeatureProvider>> {
        let shape: Vec<Retained<NSNumber>> =
            self.shape.iter().map(|&d| NSNumber::new_usize(d)).collect();
        let array = MLMultiArray::initWithShape_dataType_error(
            MLMultiArray::alloc(),
            &NSArray::from_retained_slice(&shape),
            MLMultiArrayDataType::Float32,
        )
        .map_err(ns_error)?;
        // A new array is contiguous and first-major (its docs), so a plain copy fills it.
        #[allow(deprecated)]
        let dst = array.dataPointer().as_ptr().cast::<f32>();
        std::ptr::copy_nonoverlapping(input.as_ptr(), dst, input.len());
        let value = MLFeatureValue::featureValueWithMultiArray(&array);
        let value: &AnyObject = value.as_ref();
        let features = NSDictionary::<NSString, AnyObject>::from_slices(&[&*self.input], &[value]);
        MLDictionaryFeatureProvider::initWithDictionary_error(
            MLDictionaryFeatureProvider::alloc(),
            &features,
        )
        .map_err(ns_error)
    }
}

/// Which hardware Core ML may use: `ARGUS_COREML_UNITS` = all (default), gpu, ane or cpu.
fn units() -> MLComputeUnits {
    match std::env::var("ARGUS_COREML_UNITS").as_deref() {
        Ok("gpu") => MLComputeUnits::CPUAndGPU,
        Ok("ane") => MLComputeUnits::CPUAndNeuralEngine,
        Ok("cpu") => MLComputeUnits::CPUOnly,
        _ => MLComputeUnits::All,
    }
}

fn ns_error(e: Retained<NSError>) -> Error {
    Error(format!("Core ML: {}", e.localizedDescription()))
}

fn load(files: &[(&str, &[u8])], shape: &[usize], input: &str, output: &str) -> Result<Model> {
    let compiled = compiled(files)?;
    autoreleasepool(|_| unsafe {
        let url = NSURL::from_directory_path(&compiled)
            .ok_or_else(|| Error(format!("Core ML: bad path {}", compiled.display())))?;
        let config = MLModelConfiguration::new();
        config.setComputeUnits(units());
        let model =
            MLModel::modelWithContentsOfURL_configuration_error(&url, &config).map_err(ns_error)?;
        Ok(Model {
            model,
            shape: shape.to_vec(),
            input: NSString::from_str(input),
            output: NSString::from_str(output),
            busy: Mutex::new(()),
            stuck: AtomicBool::new(false),
        })
    })
}

/// The compiled model (`.mlmodelc`) for this package, compiling it on first use.
fn compiled(files: &[(&str, &[u8])]) -> Result<PathBuf> {
    let mut hash = blake3::Hasher::new();
    for (name, bytes) in files {
        hash.update(name.as_bytes());
        hash.update(bytes);
    }
    let key = hash.finalize().to_hex();
    let dir = std::env::temp_dir()
        .join("argus-coreml")
        .join(&key.as_str()[..16]);
    let done = dir.join("model.mlmodelc");
    if done.is_dir() {
        return Ok(done);
    }
    let io = |e: std::io::Error| Error(format!("Core ML: {e}"));
    // Each process stages its own copy, so two compiling at once never mix files.
    let stage = dir.join(format!("stage-{}", std::process::id()));
    let package = stage.join("model.mlpackage");
    for (name, bytes) in files {
        let file = package.join(name);
        std::fs::create_dir_all(file.parent().unwrap_or(&package)).map_err(io)?;
        std::fs::write(&file, bytes).map_err(io)?;
    }
    let out = autoreleasepool(|_| unsafe {
        let url = NSURL::from_directory_path(&package)
            .ok_or_else(|| Error(format!("Core ML: bad path {}", package.display())))?;
        #[allow(deprecated)]
        let out = MLModel::compileModelAtURL_error(&url).map_err(ns_error)?;
        out.to_file_path()
            .ok_or_else(|| Error("Core ML: compiled model has no path".into()))
    })?;
    let mine = stage.join("model.mlmodelc");
    if std::fs::rename(&out, &mine).is_err() {
        copy_dir(&out, &mine).map_err(io)?;
        let _ = std::fs::remove_dir_all(&out);
    }
    // Another process may have finished first: theirs is the same model.
    if std::fs::rename(&mine, &done).is_err() && !done.is_dir() {
        return Err(Error(format!(
            "Core ML: could not place {}",
            done.display()
        )));
    }
    let _ = std::fs::remove_dir_all(&stage);
    Ok(done)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// An output array as first-major f32, whatever its strides (Core ML may pad rows) and
/// element type.
unsafe fn read(array: &MLMultiArray) -> Result<Vec<f32>> {
    let dims = |a: Retained<NSArray<NSNumber>>| -> Vec<usize> {
        a.iter().map(|n| n.as_isize().max(0) as usize).collect()
    };
    let shape = dims(array.shape());
    let strides = dims(array.strides());
    let n: usize = shape.iter().product();
    #[allow(deprecated)]
    let base = array.dataPointer().as_ptr();
    let kind = array.dataType();
    let at = |i: usize| -> f32 {
        if kind == MLMultiArrayDataType::Float16 {
            f16_to_f32(*base.cast::<u16>().add(i))
        } else if kind == MLMultiArrayDataType::Float64 {
            *base.cast::<f64>().add(i) as f32
        } else {
            *base.cast::<f32>().add(i)
        }
    };
    if kind != MLMultiArrayDataType::Float32
        && kind != MLMultiArrayDataType::Float16
        && kind != MLMultiArrayDataType::Float64
    {
        return Err(Error(format!("Core ML: output type {kind:?}")));
    }
    let mut out = Vec::with_capacity(n);
    let mut index = vec![0usize; shape.len()];
    for _ in 0..n {
        let offset: usize = index.iter().zip(&strides).map(|(i, s)| i * s).sum();
        out.push(at(offset));
        for d in (0..shape.len()).rev() {
            index[d] += 1;
            if index[d] < shape[d] {
                break;
            }
            index[d] = 0;
        }
    }
    Ok(out)
}

/// IEEE half to f32.
fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) as u32) << 31;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match exp {
        0 if frac == 0 => sign,
        0 => {
            // Subnormal: normalise it.
            let shift = frac.leading_zeros() - 21;
            sign | ((113 - shift) << 23) | (((frac << shift) & 0x3ff) << 13)
        }
        0x1f => sign | 0x7f80_0000 | (frac << 13),
        _ => sign | ((exp + 112) << 23) | (frac << 13),
    };
    f32::from_bits(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_convert() {
        assert_eq!(f16_to_f32(0x3c00), 1.0);
        assert_eq!(f16_to_f32(0xc000), -2.0);
        assert_eq!(f16_to_f32(0x3555), 0.333_251_95);
        assert_eq!(f16_to_f32(0x0001), 5.960_464_5e-8);
        assert_eq!(f16_to_f32(0x7c00), f32::INFINITY);
    }
}
