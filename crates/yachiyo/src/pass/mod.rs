//! Pass management for Intermediate Representations.

use std::ops::{Deref, DerefMut};

pub struct AnalysisRef<T: 'static>(*const T);

impl<T: 'static> AnalysisRef<T> {
  pub(super) fn new(ptr: *const T) -> Self {
    Self(ptr)
  }
}

impl<T: 'static> Deref for AnalysisRef<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    // Cache entries are boxed, so moving the map does not move the result.
    unsafe { &*self.0 }
  }
}

pub struct AnalysisRefMut<T: 'static>(*mut T);

impl<T: 'static> AnalysisRefMut<T> {
  pub(super) fn new(ptr: *mut T) -> Self {
    Self(ptr)
  }
}

impl<T: 'static> Deref for AnalysisRefMut<T> {
  type Target = T;

  fn deref(&self) -> &Self::Target {
    // Cache entries are boxed, so moving the map does not move the result.
    unsafe { &*self.0 }
  }
}

impl<T: 'static> DerefMut for AnalysisRefMut<T> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    unsafe { &mut *self.0 }
  }
}

mod back;
mod mid;
pub use back::*;
pub use mid::*;
