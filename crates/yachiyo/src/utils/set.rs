//! Set implemented by Vec.

use std::ops::{BitAnd, BitOr, BitXor, Sub};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Set<T> {
    items: Vec<T>,
}

impl<T> Default for Set<T> {
    fn default() -> Self {
        Self { items: Vec::new() }
    }
}

impl<T> Set<T> {
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

impl<T: PartialEq> Set<T> {
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
        self.items
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

impl<T: PartialEq + Clone> Set<T> {
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

impl<T: PartialEq> From<Vec<T>> for Set<T> {
    fn from(value: Vec<T>) -> Self {
        value.into_iter().collect()
    }
}

impl<T> IntoIterator for Set<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a Set<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<T: PartialEq> Extend<T> for Set<T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        for item in iter {
            self.insert(item);
        }
    }
}

impl<T: PartialEq> std::iter::FromIterator<T> for Set<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut out = Self::new();
        out.extend(iter);
        out
    }
}

impl<T: PartialEq + Clone> BitOr for &Set<T> {
    type Output = Set<T>;

    fn bitor(self, rhs: Self) -> Self::Output {
        self.union(rhs)
    }
}

impl<T: PartialEq + Clone> BitAnd for &Set<T> {
    type Output = Set<T>;

    fn bitand(self, rhs: Self) -> Self::Output {
        self.intersection(rhs)
    }
}

impl<T: PartialEq + Clone> Sub for &Set<T> {
    type Output = Set<T>;

    fn sub(self, rhs: Self) -> Self::Output {
        self.difference(rhs)
    }
}

impl<T: PartialEq + Clone> BitXor for &Set<T> {
    type Output = Set<T>;

    fn bitxor(self, rhs: Self) -> Self::Output {
        self.symmetric_difference(rhs)
    }
}

#[macro_export]
macro_rules! set {
	() => {
		$crate::utils::set::Set::new()
	};
	($($x:expr),+ $(,)?) => {{
		let mut out = $crate::utils::set::Set::new();
		$(
			out.insert($x);
		)+
		out
	}};
}

#[cfg(test)]
mod tests {
    use super::Set;

    #[test]
    fn insert_contains_remove() {
        let mut set = Set::new();
        assert!(set.insert(1));
        assert!(!set.insert(1));
        assert!(set.contains(&1));
        assert_eq!(set.len(), 1);

        assert!(set.remove(&1));
        assert!(!set.remove(&1));
        assert!(!set.contains(&1));
        assert!(set.is_empty());
    }

    #[test]
    fn set_algebra() {
        let a: Set<_> = [1, 2, 3].into_iter().collect();
        let b: Set<_> = [3, 4, 5].into_iter().collect();

        let union = &a | &b;
        let intersection = &a & &b;
        let difference = &a - &b;
        let sym_diff = &a ^ &b;

        assert_eq!(union.as_slice(), &[1, 2, 3, 4, 5]);
        assert_eq!(intersection.as_slice(), &[3]);
        assert_eq!(difference.as_slice(), &[1, 2]);
        assert_eq!(sym_diff.as_slice(), &[1, 2, 4, 5]);
    }

    #[test]
    fn replace_take_and_predicates() {
        #[derive(Clone, Debug, PartialOrd, Ord)]
        struct Item {
            id: i32,
            value: i32,
        }

        impl PartialEq for Item {
            fn eq(&self, other: &Self) -> bool {
                self.id == other.id
            }
        }

        impl Eq for Item {}

        let mut set = Set::new();
        assert_eq!(set.replace(Item { id: 1, value: 10 }), None);
        assert_eq!(
            set.replace(Item { id: 1, value: 20 }),
            Some(Item { id: 1, value: 10 })
        );

        let taken = set.take(&Item { id: 1, value: 20 });
        assert_eq!(taken, Some(Item { id: 1, value: 20 }));
        assert!(set.is_empty());

        let x: Set<_> = [1, 2].into_iter().collect();
        let y: Set<_> = [1, 2, 3].into_iter().collect();
        let z: Set<_> = [4, 5].into_iter().collect();
        assert!(x.is_subset(&y));
        assert!(y.is_superset(&x));
        assert!(x.is_disjoint(&z));
    }
}
