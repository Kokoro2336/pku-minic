//! Map utilities.

use std::{
  marker::PhantomData,
  ops::{Deref, DerefMut, Index, IndexMut},
};

/// A vector-backed map indexed by key types that can be converted into `usize`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexedMap<K: Into<usize>, V> {
  inner: Vec<V>,
  key: PhantomData<fn(K) -> K>,
}

impl<K: Into<usize>, V> IndexedMap<K, V> {
  /// Creates an empty indexed map.
  pub const fn new() -> Self {
    Self {
      inner: Vec::new(),
      key: PhantomData,
    }
  }

  /// Creates an empty indexed map with at least the specified capacity.
  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      inner: Vec::with_capacity(capacity),
      key: PhantomData,
    }
  }

  /// Returns the contained vector.
  pub fn into_inner(self) -> Vec<V> {
    self.inner
  }
}

impl<K: Into<usize>, V: Default> IndexedMap<K, V> {
  /// Returns a mutable reference to the entry for `key`, resizing with
  /// `V::default()` when needed.
  pub fn entry_mut(&mut self, key: K) -> &mut V {
    let index = key.into();
    if index >= self.inner.len() {
      self.inner.resize_with(index + 1, V::default);
    }
    &mut self.inner[index]
  }
}

impl<K: Into<usize>, V> Default for IndexedMap<K, V> {
  fn default() -> Self {
    Self::new()
  }
}

impl<K: Into<usize>, V> From<Vec<V>> for IndexedMap<K, V> {
  fn from(inner: Vec<V>) -> Self {
    Self {
      inner,
      key: PhantomData,
    }
  }
}

impl<K: Into<usize>, V> Deref for IndexedMap<K, V> {
  type Target = Vec<V>;

  fn deref(&self) -> &Self::Target {
    &self.inner
  }
}

impl<K: Into<usize>, V> DerefMut for IndexedMap<K, V> {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.inner
  }
}

impl<K: Into<usize>, V> Index<K> for IndexedMap<K, V> {
  type Output = V;

  fn index(&self, key: K) -> &Self::Output {
    &self.inner[key.into()]
  }
}

impl<K: Into<usize>, V: Default> IndexMut<K> for IndexedMap<K, V> {
  fn index_mut(&mut self, key: K) -> &mut Self::Output {
    self.entry_mut(key)
  }
}
