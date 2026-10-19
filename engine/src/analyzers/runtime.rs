//! The one model runtime (ADR 0013): tract on the CPU. Every Analyzer's model loads here,
//! once per process, so moving to another runtime later changes only this file.

use crate::{Error, Result};
use std::sync::OnceLock;
use tract_onnx::prelude::*;

pub use tract_onnx::prelude::{tvec, Tensor, TractError};
pub use tract_onnx::tract_core::ndarray;

pub type Model = std::sync::Arc<TypedRunnableModel>;

/// An embedded ONNX model with a fixed input shape, loaded on first use.
pub struct Lazy {
    bytes: &'static [u8],
    shape: &'static [usize],
    model: OnceLock<std::result::Result<Model, String>>,
}

impl Lazy {
    pub const fn new(bytes: &'static [u8], shape: &'static [usize]) -> Self {
        Self {
            bytes,
            shape,
            model: OnceLock::new(),
        }
    }

    pub fn get(&self) -> Result<&Model> {
        self.model
            .get_or_init(|| load(self.bytes, self.shape).map_err(|e| e.0))
            .as_ref()
            .map_err(|e| Error(e.clone()))
    }

    /// Run on one f32 input, returning every output as a plain f32 tensor.
    pub fn run(&self, input: Tensor) -> Result<Vec<Tensor>> {
        let out = self
            .get()?
            .run(tvec!(input.into()))
            .map_err(|e| Error(format!("model: {e}")))?;
        Ok(out.into_iter().map(|t| t.into_tensor()).collect())
    }
}

fn load(bytes: &[u8], shape: &[usize]) -> Result<Model> {
    let err = |e: TractError| Error(format!("model: {e}"));
    tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(bytes))
        .map_err(err)?
        .with_input_fact(0, f32::fact(shape).into())
        .map_err(err)?
        .into_optimized()
        .map_err(err)?
        .into_runnable()
        .map_err(err)
}

/// A model output as a flat f32 slice.
pub fn floats(t: &Tensor) -> Result<&[f32]> {
    t.try_as_plain_ram()
        .and_then(|v| v.as_slice::<f32>())
        .map_err(|e| Error(format!("model output: {e}")))
}
