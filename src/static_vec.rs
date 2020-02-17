use std::marker::PhantomData;

/// A vector that doesn't change size, so all references (IDs) are always valid.
#[derive(Clone)]
pub struct StaticVec<I, T> {
    data: Vec<T>,
    marker: PhantomData<I>,
}

pub trait Id: Into<usize> + From<usize> {}

impl<I, T> StaticVec<I, T> {
    pub fn new(data: Vec<T>) -> Self {
        Self {
            data,
            marker: PhantomData::default(),
        }
    }

    pub fn map<S, F: Fn(&T) -> S>(&self, f: F) -> StaticVec<I, S> {
        StaticVec::new(self.data.iter().map(f).collect())
    }

    pub fn num_elements(&self) -> usize {
        self.data.len()
    }
}

impl<I, T: Clone + Default> StaticVec<I, T> {
    pub fn new_with_default(num_elements: usize) -> Self {
        StaticVec::new(vec![T::default(); num_elements])
    }
}

impl<I, T: Clone> StaticVec<I, T> {
    pub fn fill(value: T, num_elements: usize) -> Self {
        StaticVec::new(vec![value; num_elements])
    }
}

impl<I: Id, T> StaticVec<I, T> {
    pub fn get(&self, id: I) -> &T {
        &self.data[id.into()]
    }

    pub fn get_mut(&mut self, id: I) -> &mut T {
        &mut self.data[id.into()]
    }

    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.data.iter().enumerate().map(|(i, d)| (I::from(i), d))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (I, &mut T)> {
        self.data
            .iter_mut()
            .enumerate()
            .map(|(i, d)| (I::from(i), d))
    }
}
