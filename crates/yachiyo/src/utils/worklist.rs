//! Worklist implementation.

use crate::utils::BitSet;

use rustc_hash::FxHashSet;
use std::collections::VecDeque;
use std::hash::Hash;

#[derive(Debug)]
pub struct Worklist<T, S>
where
  Self: WorklistTrait<T>,
{
  list: VecDeque<T>,
  in_list: S,
}

impl<T, S: Default> Default for Worklist<T, S>
where
  Self: WorklistTrait<T>,
{
  fn default() -> Self {
    Self {
      list: VecDeque::new(),
      in_list: S::default(),
    }
  }
}

impl<T, S: Default> Worklist<T, S>
where
  Self: WorklistTrait<T>,
{
  pub fn new() -> Self {
    Self::default()
  }
}

impl<T, S> Worklist<T, S>
where
  Self: WorklistTrait<T>,
{
  pub fn get_in_list(&self) -> &S {
    &self.in_list
  }

  pub fn push_back(&mut self, item: T) {
    <Self as WorklistTrait<T>>::push_back(self, item);
  }

  pub fn pop_back(&mut self) -> Option<T> {
    <Self as WorklistTrait<T>>::pop_back(self)
  }

  pub fn push_front(&mut self, item: T) {
    <Self as WorklistTrait<T>>::push_front(self, item);
  }

  pub fn pop_front(&mut self) -> Option<T> {
    <Self as WorklistTrait<T>>::pop_front(self)
  }

  pub fn is_empty(&self) -> bool {
    <Self as WorklistTrait<T>>::is_empty(self)
  }

  pub fn len(&self) -> usize {
    <Self as WorklistTrait<T>>::len(self)
  }

  pub fn clear(&mut self) {
    <Self as WorklistTrait<T>>::clear(self);
  }

  pub fn contains(&self, item: &T) -> bool {
    <Self as WorklistTrait<T>>::contains(self, item)
  }

  pub fn remove(&mut self, item: &T) -> bool {
    <Self as WorklistTrait<T>>::remove(self, item)
  }
}

#[allow(unused)]
pub trait WorklistTrait<T> {
  fn push_back(&mut self, item: T);
  fn pop_back(&mut self) -> Option<T>;
  fn push_front(&mut self, item: T);
  fn pop_front(&mut self) -> Option<T>;
  fn is_empty(&self) -> bool;
  fn len(&self) -> usize;
  fn clear(&mut self);
  fn contains(&self, item: &T) -> bool;
  fn remove(&mut self, item: &T) -> bool;
}

/// If the items can not be easily converted to usize and hashable, we use a hash set to track membership.
impl<T: Eq + Hash + Clone> WorklistTrait<T> for Worklist<T, FxHashSet<T>> {
  fn push_back(&mut self, item: T) {
    if self.in_list.insert(item.clone()) {
      self.list.push_back(item);
    }
  }

  fn pop_back(&mut self) -> Option<T> {
    if let Some(item) = self.list.pop_back() {
      self.in_list.remove(&item);
      Some(item)
    } else {
      None
    }
  }

  fn push_front(&mut self, item: T) {
    if self.in_list.insert(item.clone()) {
      self.list.push_front(item);
    }
  }

  fn pop_front(&mut self) -> Option<T> {
    if let Some(item) = self.list.pop_front() {
      self.in_list.remove(&item);
      Some(item)
    } else {
      None
    }
  }

  fn is_empty(&self) -> bool {
    self.list.is_empty()
  }

  fn len(&self) -> usize {
    self.list.len()
  }

  fn clear(&mut self) {
    self.list.clear();
    self.in_list.clear();
  }

  fn contains(&self, item: &T) -> bool {
    self.in_list.contains(item)
  }

  fn remove(&mut self, item: &T) -> bool {
    if self.in_list.remove(item) {
      self
        .list
        .remove(self.list.iter().position(|x| x == item).unwrap());
      true
    } else {
      false
    }
  }
}

/// If the items can be easily converted to usize, we can use a bitset to track membership for better performance.
impl<T: Into<usize> + PartialEq + Clone> WorklistTrait<T> for Worklist<T, BitSet> {
  fn push_back(&mut self, item: T) {
    let index = item.clone().into();
    if !self.in_list.contains(index) {
      self.in_list.insert(index);
      self.list.push_back(item);
    }
  }

  fn pop_back(&mut self) -> Option<T> {
    if let Some(item) = self.list.pop_back() {
      let index = item.clone().into();
      self.in_list.remove(index);
      Some(item)
    } else {
      None
    }
  }

  fn push_front(&mut self, item: T) {
    let index = item.clone().into();
    if !self.in_list.contains(index) {
      self.in_list.insert(index);
      self.list.push_front(item);
    }
  }

  fn pop_front(&mut self) -> Option<T> {
    if let Some(item) = self.list.pop_front() {
      let index = item.clone().into();
      self.in_list.remove(index);
      Some(item)
    } else {
      None
    }
  }

  fn is_empty(&self) -> bool {
    self.list.is_empty()
  }

  fn len(&self) -> usize {
    self.list.len()
  }

  fn clear(&mut self) {
    self.list.clear();
    self.in_list.clear();
  }

  fn contains(&self, item: &T) -> bool {
    let index = item.clone().into();
    self.in_list.contains(index)
  }

  fn remove(&mut self, item: &T) -> bool {
    if self.in_list.remove(item.clone().into()) {
      self
        .list
        .remove(self.list.iter().position(|x| x == item).unwrap());
      true
    } else {
      false
    }
  }
}
