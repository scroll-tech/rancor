//! # rancor
//!
//! rancor provides scalable and efficient error handling without using type
//! composition. This makes it best-suited for situations where:
//!
//! - Programmatic error introspection is not useful
//! - Functions may error, but succeed most of the time
//! - Errors should provide as much useful detail as possible when emitted
//! - Use cases include both `no_std` and targets with support for `std`

#![deny(
    future_incompatible,
    missing_docs,
    nonstandard_style,
    unsafe_op_in_unsafe_fn,
    unused,
    warnings,
    clippy::all,
    clippy::missing_safety_doc,
    clippy::undocumented_unsafe_blocks,
    rustdoc::broken_intra_doc_links,
    rustdoc::missing_crate_level_docs
)]
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
mod boxed_error;
#[cfg(feature = "alloc")]
mod thin_box;

use core::{
    fmt,
    hint::unreachable_unchecked,
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

#[cfg(feature = "std")]
use std::error::Error as StdError;

#[cfg(not(feature = "std"))]
/// An error that can be debugged and displayed.
///
/// Without the `std` feature enabled, this has supertraits of
/// [`core::fmt::Debug`] and [`core::fmt::Display`]. With the `std`
/// feature enabled, this also has a supertrait of [`std::error::Error`]
/// instead.
///
/// This trait is always `Send + Sync + 'static`.
#[cfg_attr(feature = "alloc", ptr_meta::pointee)]
pub trait StdError: fmt::Debug + fmt::Display {
    /// The lower-level source of this error, if any.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        None
    }
}

#[cfg(not(feature = "std"))]
impl<T: fmt::Debug + fmt::Display + ?Sized> StdError for T {}

/// A type which can add an additional trace to itself.
pub trait Trace: Sized + Send + Sync + 'static {
    /// Adds an additional trace to this error, returning a new error.
    fn trace<R>(self, trace: R) -> Self
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static;
}

/// An error type which can be uniformly constructed from a [`StdError`] and
/// additional trace information.
pub trait Error: Trace + StdError {
    /// Returns a new `Self` using the given [`Error`].
    ///
    /// Depending on the specific implementation, this may box the error,
    /// immediately emit a diagnostic, or discard it and only remember that some
    /// error occurred.
    fn new<T: StdError + Send + Sync + 'static>(source: T) -> Self;
}

/// A type with fallible operations that return its associated error type.
pub trait Fallible {
    /// The error type associated with this type's operations.
    type Error;
}

/// Equips a type with a `Fallible` implementation that chooses `E` as its error
/// type.
#[repr(transparent)]
pub struct Strategy<T: ?Sized, E> {
    _error: PhantomData<E>,
    inner: T,
}

impl<T: ?Sized, E> Fallible for Strategy<T, E> {
    type Error = E;
}

impl<T: ?Sized, E> Strategy<T, E> {
    /// Wraps the given mutable reference, returning a mutable reference to a
    /// `Strategy`.
    pub fn wrap(inner: &mut T) -> &mut Self {
        // SAFETY: `Strategy` is `repr(transparent)` and so has the same layout
        // as `T`. The input and output lifetimes are the same, so mutable
        // aliasing rules will be upheld. Finally, because the inner `T` is the
        // final element of `Strategy`, the pointer metadata of the two pointers
        // will be the same.
        unsafe { core::mem::transmute::<&mut T, &mut Self>(inner) }
    }
}

impl<T: ?Sized, E> Deref for Strategy<T, E> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: ?Sized, E> DerefMut for Strategy<T, E> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

/// Returns the given error from this function.
#[macro_export]
macro_rules! fail {
    ($($x:tt)*) => {
        return ::core::result::Result::Err($crate::Error::new($($x)*))
    };
}

/// Helper methods for `Result`s.
pub trait ResultExt<T, E> {
    /// Returns a `Result` with this error type converted to `U`.
    fn into_error<U>(self) -> Result<T, U>
    where
        U: Error,
        E: StdError + Send + Sync + 'static;

    /// Returns a `Result` with this error type converted to `U` and with an
    /// additional `trace` message added.
    fn into_trace<U, R>(self, trace: R) -> Result<T, U>
    where
        U: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        E: StdError + Send + Sync + 'static;

    /// Returns a `Result` with this error type converted to `U` and with an
    /// additional trace message added by evaluating the given function `f`. The
    /// function is evaluated only if an error occurred.
    fn into_with_trace<U, R, F>(self, f: F) -> Result<T, U>
    where
        U: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R,
        E: StdError + Send + Sync + 'static;

    /// Adds an additional `trace` message to the error value of this type.
    fn trace<R>(self, trace: R) -> Result<T, E>
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        E: Trace;

    /// Adds an additional trace message to the error value of this type by
    /// evaluating the given function `f`. The function is evaluated only if an
    /// error occurred.
    fn with_trace<R, F>(self, f: F) -> Result<T, E>
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R,
        E: Trace;
}

impl<T, E> ResultExt<T, E> for Result<T, E> {
    fn into_error<U>(self) -> Result<T, U>
    where
        U: Error,
        E: StdError + Send + Sync + 'static,
    {
        match self {
            Ok(x) => Ok(x),
            Err(e) => Err(U::new(e)),
        }
    }

    fn into_trace<U, R>(self, trace: R) -> Result<T, U>
    where
        U: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        E: StdError + Send + Sync + 'static,
    {
        match self {
            Ok(x) => Ok(x),
            Err(e) => Err(U::new(e).trace(trace)),
        }
    }

    fn into_with_trace<U, R, F>(self, f: F) -> Result<T, U>
    where
        U: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R,
        E: StdError + Send + Sync + 'static,
    {
        match self {
            Ok(x) => Ok(x),
            Err(e) => Err(U::new(e).trace(f())),
        }
    }

    fn trace<R>(self, trace: R) -> Result<T, E>
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        E: Trace,
    {
        match self {
            Ok(x) => Ok(x),
            Err(e) => Err(e.trace(trace)),
        }
    }

    fn with_trace<R, F>(self, f: F) -> Result<T, E>
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R,
        E: Trace,
    {
        match self {
            Ok(x) => Ok(x),
            Err(e) => Err(e.trace(f())),
        }
    }
}

/// Helper methods for `Option`s.
pub trait OptionExt<T> {
    /// Returns a `Result` with this error type converted to `U`.
    fn into_error<E>(self) -> Result<T, E>
    where
        E: Error;

    /// Returns a `Result` with this error type converted to `U` and with an
    /// additional `trace` message added.
    fn into_trace<E, R>(self, trace: R) -> Result<T, E>
    where
        E: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static;

    /// Returns a `Result` with this error type converted to `U` and with an
    /// additional trace message added by evaluating the given function `f`. The
    /// function is evaluated only if an error occurred.
    fn into_with_trace<E, R, F>(self, f: F) -> Result<T, E>
    where
        E: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R;
}

#[derive(Debug)]
struct NoneError;

impl fmt::Display for NoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`Option` is `None`, expected `Some`")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for NoneError {}

impl<T> OptionExt<T> for Option<T> {
    fn into_error<E>(self) -> Result<T, E>
    where
        E: Error,
    {
        match self {
            Some(x) => Ok(x),
            None => Err(E::new(NoneError)),
        }
    }

    fn into_trace<E, R>(self, trace: R) -> Result<T, E>
    where
        E: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
    {
        match self {
            Some(x) => Ok(x),
            None => Err(E::new(NoneError).trace(trace)),
        }
    }

    fn into_with_trace<E, R, F>(self, f: F) -> Result<T, E>
    where
        E: Error,
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
        F: FnOnce() -> R,
    {
        match self {
            Some(x) => Ok(x),
            None => Err(E::new(NoneError).trace(f())),
        }
    }
}

pub use core::convert::Infallible;

impl Trace for Infallible {
    fn trace<R>(self, _: R) -> Self
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
    {
        // SAFETY: `Infallible` is an enum with no variants, and so can never be
        // constructed as the `self` parameter.
        unsafe {
            unreachable_unchecked();
        }
    }
}

/// An error type that does not occupy any space, panicking on creation instead.
#[derive(Debug)]
pub enum Panic {}

impl fmt::Display for Panic {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        // SAFETY: `Panic` is an enum with no variants, and so can never be
        // constructed as the `self` parameter.
        unsafe {
            unreachable_unchecked();
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Panic {}

impl Trace for Panic {
    fn trace<R>(self, _: R) -> Self
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
    {
        // SAFETY: `Panic` is an enum with no variants, and so can never be
        // constructed as the `self` parameter.
        unsafe {
            unreachable_unchecked();
        }
    }
}

impl Error for Panic {
    fn new<T: fmt::Display>(error: T) -> Self {
        panic!("created a new `Panic` from: {error}");
    }
}

/// An error type that only preserves success or failure, throwing away any more
/// detailed error messages.
#[derive(Debug)]
pub struct Failure;

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to check bytes")
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Failure {}

impl Trace for Failure {
    fn trace<R>(self, _: R) -> Self
    where
        R: fmt::Debug + fmt::Display + Send + Sync + 'static,
    {
        self
    }
}

impl Error for Failure {
    fn new<T: StdError + Send + Sync + 'static>(_: T) -> Self {
        Self
    }
}

#[cfg(feature = "alloc")]
pub use boxed_error::BoxedError;
