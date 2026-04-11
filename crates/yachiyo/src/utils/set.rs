//! Set Implementations.

use std::fmt;
use std::ops::{
  BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Index, IndexMut, Sub, SubAssign,
};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArraySet<T> {
  items: Vec<T>,
}

impl<T> Default for ArraySet<T> {
  fn default() -> Self {
    Self { items: Vec::new() }
  }
}

impl<T> ArraySet<T> {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_capacity(capacity: usize) -> Self {
    Self {
      items: Vec::with_capacity(capacity),
    }
  }

  pub fn len(&self) -> usize {
    self.items.len()
  }

  pub fn is_empty(&self) -> bool {
    self.items.is_empty()
  }

  pub fn capacity(&self) -> usize {
    self.items.capacity()
  }

  pub fn reserve(&mut self, additional: usize) {
    self.items.reserve(additional);
  }

  pub fn clear(&mut self) {
    self.items.clear();
  }

  pub fn retain<F>(&mut self, mut f: F)
  where
    F: FnMut(&T) -> bool,
  {
    self.items.retain(|item| f(item));
  }

  pub fn first(&self) -> Option<&T> {
    self.items.first()
  }

  pub fn last(&self) -> Option<&T> {
    self.items.last()
  }

  pub fn pop(&mut self) -> Option<T> {
    self.items.pop()
  }

  pub fn iter(&self) -> std::slice::Iter<'_, T> {
    self.items.iter()
  }

  pub fn as_slice(&self) -> &[T] {
    &self.items
  }
}

impl<T: PartialEq> ArraySet<T> {
  pub fn contains(&self, value: &T) -> bool {
    self.items.contains(value)
  }

  pub fn get(&self, value: &T) -> Option<&T> {
    self.items.iter().find(|item| *item == value)
  }

  pub fn insert(&mut self, value: T) -> bool {
    if self.contains(&value) {
      false
    } else {
      self.items.push(value);
      true
    }
  }

  pub fn replace(&mut self, value: T) -> Option<T> {
    if let Some(index) = self.items.iter().position(|item| *item == value) {
      Some(std::mem::replace(&mut self.items[index], value))
    } else {
      self.items.push(value);
      None
    }
  }

  pub fn remove(&mut self, value: &T) -> bool {
    if let Some(index) = self.items.iter().position(|item| item == value) {
      self.items.remove(index);
      true
    } else {
      false
    }
  }

  pub fn take(&mut self, value: &T) -> Option<T> {
    self
      .items
      .iter()
      .position(|item| item == value)
      .map(|index| self.items.remove(index))
  }

  pub fn is_subset(&self, other: &Self) -> bool {
    self.iter().all(|item| other.contains(item))
  }

  pub fn is_superset(&self, other: &Self) -> bool {
    other.is_subset(self)
  }

  pub fn is_disjoint(&self, other: &Self) -> bool {
    self.iter().all(|item| !other.contains(item))
  }
}

impl<T: PartialEq + Clone> ArraySet<T> {
  pub fn union(&self, other: &Self) -> Self {
    let mut out = Self::with_capacity(self.len() + other.len());
    out.extend(self.iter().cloned());
    out.extend(other.iter().cloned());
    out
  }

  pub fn intersection(&self, other: &Self) -> Self {
    let mut out = Self::new();
    for item in self.iter() {
      if other.contains(item) {
        out.insert(item.clone());
      }
    }
    out
  }

  pub fn difference(&self, other: &Self) -> Self {
    let mut out = Self::new();
    for item in self.iter() {
      if !other.contains(item) {
        out.insert(item.clone());
      }
    }
    out
  }

  pub fn symmetric_difference(&self, other: &Self) -> Self {
    let mut out = Self::new();
    out.extend(self.iter().filter(|item| !other.contains(item)).cloned());
    out.extend(other.iter().filter(|item| !self.contains(item)).cloned());
    out
  }
}

impl<T: PartialEq> From<Vec<T>> for ArraySet<T> {
  fn from(value: Vec<T>) -> Self {
    value.into_iter().collect()
  }
}

impl<T> IntoIterator for ArraySet<T> {
  type Item = T;
  type IntoIter = std::vec::IntoIter<T>;

  fn into_iter(self) -> Self::IntoIter {
    self.items.into_iter()
  }
}

impl<'a, T> IntoIterator for &'a ArraySet<T> {
  type Item = &'a T;
  type IntoIter = std::slice::Iter<'a, T>;

  fn into_iter(self) -> Self::IntoIter {
    self.items.iter()
  }
}

impl<T: PartialEq> Extend<T> for ArraySet<T> {
  fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
    for item in iter {
      self.insert(item);
    }
  }
}

impl<T: PartialEq> std::iter::FromIterator<T> for ArraySet<T> {
  fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
    let mut out = Self::new();
    out.extend(iter);
    out
  }
}

impl<T: PartialEq + Clone> BitOr for &ArraySet<T> {
  type Output = ArraySet<T>;

  fn bitor(self, rhs: Self) -> Self::Output {
    self.union(rhs)
  }
}

impl<T: PartialEq + Clone> BitAnd for &ArraySet<T> {
  type Output = ArraySet<T>;

  fn bitand(self, rhs: Self) -> Self::Output {
    self.intersection(rhs)
  }
}

impl<T: PartialEq + Clone> Sub for &ArraySet<T> {
  type Output = ArraySet<T>;

  fn sub(self, rhs: Self) -> Self::Output {
    self.difference(rhs)
  }
}

impl<T: PartialEq + Clone> BitXor for &ArraySet<T> {
  type Output = ArraySet<T>;

  fn bitxor(self, rhs: Self) -> Self::Output {
    self.symmetric_difference(rhs)
  }
}

#[macro_export]
macro_rules! array_set {
	() => {
		$crate::utils::set::ArraySet::new()
	};
	($($x:expr),+ $(,)?) => {{
		let mut out = $crate::utils::set::ArraySet::new();
		$(
			out.insert($x);
		)+
		out
	}};
}

pub use array_set;

#[derive(Clone, PartialEq, Eq, Hash, Default)]
pub struct BitSet {
  bits: Vec<u64>,
}

impl BitSet {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn with_capacity(nbits: usize) -> Self {
    let len = nbits.div_ceil(64);
    Self {
      bits: Vec::with_capacity(len),
    }
  }

  pub fn insert(&mut self, bit: usize) -> bool {
    let idx = bit / 64;
    let offset = bit % 64;
    if idx >= self.bits.len() {
      self.bits.resize(idx + 1, 0);
    }
    let mask = 1 << offset;
    let present = (self.bits[idx] & mask) != 0;
    self.bits[idx] |= mask;
    !present
  }

  pub fn remove(&mut self, bit: usize) -> bool {
    let idx = bit / 64;
    if idx >= self.bits.len() {
      return false;
    }
    let offset = bit % 64;
    let mask = 1 << offset;
    let present = (self.bits[idx] & mask) != 0;
    self.bits[idx] &= !mask;
    present
  }

  pub fn contains(&self, bit: usize) -> bool {
    let idx = bit / 64;
    if idx >= self.bits.len() {
      return false;
    }
    let offset = bit % 64;
    (self.bits[idx] & (1 << offset)) != 0
  }

  pub fn clear(&mut self) {
    self.bits.clear();
  }

  pub fn len(&self) -> usize {
    self.bits.iter().map(|&w| w.count_ones() as usize).sum()
  }

  pub fn is_empty(&self) -> bool {
    self.bits.iter().all(|&w| w == 0)
  }

  pub fn capacity(&self) -> usize {
    self.bits.len() * 64
  }

  pub fn num_words(&self) -> usize {
    self.bits.len()
  }
}

impl<Idx> Index<Idx> for BitSet
where
  Idx: Into<usize>,
{
  type Output = u64;

  fn index(&self, index: Idx) -> &Self::Output {
    &self.bits[index.into()]
  }
}

impl<Idx> IndexMut<Idx> for BitSet
where
  Idx: Into<usize>,
{
  fn index_mut(&mut self, index: Idx) -> &mut Self::Output {
    let index = index.into();
    if index >= self.bits.len() {
      self.bits.resize(index + 1, 0);
    }
    &mut self.bits[index]
  }
}

// Metaprogramming: Macro to implement binary operators
macro_rules! impl_bitop {
    ($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident, $op:tt) => {
        impl $trait for &BitSet {
            type Output = BitSet;

            fn $method(self, rhs: Self) -> Self::Output {
                let len = std::cmp::max(self.bits.len(), rhs.bits.len());
                let mut new_bits = Vec::with_capacity(len);
                for i in 0..len {
                    let lhs_word = self.bits.get(i).copied().unwrap_or(0);
                    let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
                    new_bits.push(lhs_word $op rhs_word);
                }
                // Trim trailing zeros? Optional, but good for equality checks
                while new_bits.last() == Some(&0) {
                    new_bits.pop();
                }
                BitSet { bits: new_bits }
            }
        }

        impl $assign_trait for BitSet {
            fn $assign_method(&mut self, rhs: Self) {
                let len = std::cmp::max(self.bits.len(), rhs.bits.len());
                if self.bits.len() < len {
                    self.bits.resize(len, 0);
                }
                for i in 0..len {
                    let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
                    self.bits[i] = self.bits[i] $op rhs_word;
                }
                while self.bits.last() == Some(&0) {
                    self.bits.pop();
                }
            }
        }

        impl $assign_trait<&BitSet> for BitSet {
            fn $assign_method(&mut self, rhs: &BitSet) {
                let len = std::cmp::max(self.bits.len(), rhs.bits.len());
                if self.bits.len() < len {
                    self.bits.resize(len, 0);
                }
                for i in 0..len {
                    let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
                    self.bits[i] = self.bits[i] $op rhs_word;
                }
                while self.bits.last() == Some(&0) {
                    self.bits.pop();
                }
            }
        }
    };
}

impl_bitop!(BitAnd, bitand, BitAndAssign, bitand_assign, &);
impl_bitop!(BitOr, bitor, BitOrAssign, bitor_assign, |);
impl_bitop!(BitXor, bitxor, BitXorAssign, bitxor_assign, ^);

// Difference is slightly different (lhs & !rhs), so we impl manually or make macro more generic.
// But set difference usually means remove items in rhs from lhs.
impl Sub for &BitSet {
  type Output = BitSet;

  fn sub(self, rhs: Self) -> Self::Output {
    let len = self.bits.len();
    let mut new_bits = Vec::with_capacity(len);
    for i in 0..len {
      let lhs_word = self.bits[i];
      let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
      new_bits.push(lhs_word & !rhs_word);
    }
    while new_bits.last() == Some(&0) {
      new_bits.pop();
    }
    BitSet { bits: new_bits }
  }
}

impl SubAssign for BitSet {
  fn sub_assign(&mut self, rhs: Self) {
    for i in 0..self.bits.len() {
      let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
      self.bits[i] &= !rhs_word;
    }
    while self.bits.last() == Some(&0) {
      self.bits.pop();
    }
  }
}

impl SubAssign<&BitSet> for BitSet {
  fn sub_assign(&mut self, rhs: &BitSet) {
    for i in 0..self.bits.len() {
      let rhs_word = rhs.bits.get(i).copied().unwrap_or(0);
      self.bits[i] &= !rhs_word;
    }
    while self.bits.last() == Some(&0) {
      self.bits.pop();
    }
  }
}

impl fmt::Debug for BitSet {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_list().entries(self.iter()).finish()
  }
}

// Iterator support
pub struct Iter<'a> {
  bitset: &'a BitSet,
  idx: usize,
}

impl<'a> Iterator for Iter<'a> {
  type Item = usize;

  fn next(&mut self) -> Option<Self::Item> {
    while self.idx / 64 < self.bitset.bits.len() {
      let word_idx = self.idx / 64;
      let bit_idx = self.idx % 64;
      let word = self.bitset.bits[word_idx];

      // Optimization: skip empty words
      if word == 0 {
        self.idx = (word_idx + 1) * 64;
        continue;
      }

      // Check bits in current word
      if (word & (1 << bit_idx)) != 0 {
        let ret = self.idx;
        self.idx += 1;
        return Some(ret);
      }

      // Find next set bit efficiently
      // Mask out bits before current bit_idx
      let masked_word = word & (!0 << bit_idx);
      if masked_word != 0 {
        let next_bit = masked_word.trailing_zeros() as usize;
        self.idx = word_idx * 64 + next_bit + 1;
        return Some(word_idx * 64 + next_bit);
      } else {
        self.idx = (word_idx + 1) * 64;
      }
    }
    None
  }
}

impl BitSet {
  pub fn iter(&self) -> Iter<'_> {
    Iter {
      bitset: self,
      idx: 0,
    }
  }
}

// Metaprogramming: Initialization macro
#[macro_export]
macro_rules! bitset {
    () => {
        $crate::utils::set::BitSet::new()
    };
    ($($x:expr),+ $(,)?) => {
        {
            let mut bs = $crate::utils::set::BitSet::new();
            $(
                bs.insert($x);
            )+
            bs
        }
    };
}

pub use bitset;
