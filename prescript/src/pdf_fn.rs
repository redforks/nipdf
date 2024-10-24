//! Implement Pdf Type4 PostScript function
use crate::machine::{Machine, MachineError};

pub struct PdfFunc {
    script: Box<[u8]>,
    n_out: usize,
}

impl PdfFunc {
    /// Create a new PdfFunc.
    /// `script`: PostScript script.
    /// `n_out`: number of return value.
    pub fn new(script: impl Into<Box<[u8]>>, n_out: usize) -> Self {
        Self {
            script: script.into(),
            n_out,
        }
    }

    /// Execute the function.
    ///
    /// `args` pushed to stack before execution.
    /// return numbers of stack after execution.
    pub fn exec(&self, args: &[f32]) -> Result<Vec<f32>, MachineError> {
        let mut m = Machine::new(self.script.as_ref());
        m.exec_as_function(args, self.n_out)
    }
}

#[cfg(test)]
mod tests;
