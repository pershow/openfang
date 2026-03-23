//! Kernel-specific error types.

use openparlant_types::error::SiliCrewError;
use thiserror::Error;

/// Kernel error type wrapping SiliCrewError with kernel-specific context.
#[derive(Error, Debug)]
pub enum KernelError {
    /// A wrapped SiliCrewError.
    #[error(transparent)]
    OpenParlant(#[from] SiliCrewError),

    /// The kernel failed to boot.
    #[error("Boot failed: {0}")]
    BootFailed(String),
}

/// Alias for kernel results.
pub type KernelResult<T> = Result<T, KernelError>;
