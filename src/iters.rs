// This file is part of faster, the SIMD library for humans.
// Copyright 2017 Adam Niederer <adam.niederer@gmail.com>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use vecs::{Packable, Packed};
use intrin::Merge;
use core_or_std::slice::from_raw_parts;

/// An iterator which automatically packs the values it iterates over into SIMD
/// vectors.
pub trait SIMDIterator : Sized + ExactSizeIterator {
    type Scalar : Packable;
    type Vector : Packed<Scalar = Self::Scalar>;

    #[inline(always)]
    fn width(&self) -> usize {
        Self::Vector::WIDTH
    }

    /// Return the length of this iterator, measured in scalar elements.
    fn scalar_len(&self) -> usize;

    /// Return the current position of this iterator, measured in scalar
    /// elements.
    fn scalar_position(&self) -> usize;

    /// Pack and return a vector containing the next `self.width()` elements
    /// of the iterator, or return None if there aren't enough elements left
    fn next_vector(&mut self) -> Option<Self::Vector>;

    /// Pack and return a partially full vector containing up to the next
    /// `self.width()` of the iterator, or None if no elements are left.
    /// Elements which are not filled are instead initialized to default.
    fn next_partial(&mut self, default: Self::Vector) -> Option<(Self::Vector, usize)>;

    #[inline(always)]
    /// Return an iterator which calls `func` on vectors of elements.
    fn simd_map<A, B, F>(self, default: Self::Vector, func: F) -> SIMDMap<Self, F>
        where F : FnMut(Self::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
        SIMDMap {
            iter: self,
            func: func,
            default: default
        }
    }

    #[inline(always)]
    /// Pack and run `func` over the iterator, returning no value and not
    /// modifying the iterator.
    fn simd_do_each<F>(&mut self, default: Self::Vector, mut func: F)
        where F : FnMut(Self::Vector) -> () {
        while let Some(v) = self.next_vector() {
            func(v);
        }
        if let Some((v, _)) = self.next_partial(default) {
            func(v);
        }
    }

    #[inline(always)]
    /// Return a vector generated by reducing `func` over accumulator `start`
    /// and the values of this iterator, initializing all vectors to `default`
    /// before populating them with elements of the iterator.
    ///
    /// # Examples
    ///
    /// ```
    /// extern crate faster;
    /// use faster::*;
    ///
    /// # fn main() {
    /// let reduced = (&[2.0f32; 100][..]).simd_iter()
    ///    .simd_reduce(f32s::splat(0.0), f32s::splat(0.0), |acc, v| acc + v);
    /// # }
    /// ```
    ///
    /// In this example, on a machine with 4-element vectors, the argument to
    /// the last call of the closure is
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 50.0 | 50.0 | 50.0 | 50.0 ]
    /// ```
    ///
    /// whereas on a machine with 8-element vectors, the last call is passed
    ///
    /// ```rust,ignore
    /// [ 2.0 | 2.0 | 2.0 | 2.0 | 0.0 | 0.0 | 0.0 | 0.0 ]
    /// ```
    ///
    /// and the result of the reduction is
    ///
    /// ```rust,ignore
    /// [ 26.0 | 26.0 | 26.0 | 26.0 | 24.0 | 24.0 | 24.0 | 24.0 ]
    /// ```
    ///
    /// # Footgun Warning
    ///
    /// The results of `simd_reduce` are not portable, and it is your
    /// responsibility to interpret the result in such a way that the it is
    /// consistent across different architectures. See [`Packed::sum`] and
    /// [`Packed::product`] for built-in functions which may be helpful.
    ///
    /// [`Packed::sum`]: vecs/trait.Packed.html#tymethod.sum
    /// [`Packed::product`]: vecs/trait.Packed.html#tymethod.product
    fn simd_reduce<A, F>(&mut self, start: A, default: Self::Vector, mut func: F) -> A
        where F : FnMut(A, Self::Vector) -> A {
        let mut acc: A;
        if let Some(v) = self.next_vector() {
            acc = func(start, v);
            while let Some(v) = self.next_vector() {
                acc = func(acc, v);
            }
            if let Some((v, _)) = self.next_partial(default) {
                acc = func(acc, v);
            }
            debug_assert!(self.next_partial(default).is_none());
            acc
        } else if let Some((v, _)) = self.next_partial(default) {
            acc = func(start, v);
            debug_assert!(self.next_partial(default).is_none());
            acc
        } else {
            start
        }
    }

    /// Create a PackedIter over the remaining elements in this iterator
    #[inline(always)]
    fn pack(self) -> PackedIter<Self> {
        PackedIter {
            iter: self,
        }
    }
}

pub trait SIMDArray : SIMDIterator {
    fn load(&self, offset: usize) -> Self::Vector;
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector;
    fn load_scalar(&self, offset: usize) -> Self::Scalar;
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar;
}

pub trait SIMDArrayMut : SIMDArray {
    fn store(&mut self, value: Self::Vector, offset: usize);
    unsafe fn store_unchecked(&mut self, value: Self::Vector, offset: usize);
    fn store_scalar(&mut self, value: Self::Scalar, offset: usize);
    unsafe fn store_scalar_unchecked(&mut self, value: Self::Scalar, offset: usize);
}

/// A slice-backed iterator which can automatically pack its constituent
/// elements into vectors.
#[derive(Debug)]
pub struct SIMDRefIter<'a, T : 'a + Packable> {
    pub position: usize,
    pub data: &'a [T],
}

/// A slice-backed iterator which can automatically pack its constituent
/// elements into vectors.
#[derive(Debug)]
pub struct SIMDRefMutIter<'a, T : 'a + Packable> {
    pub position: usize,
    pub data: &'a mut [T],
}

/// A slice-backed iterator which can automatically pack its constituent
/// elements into vectors.
#[derive(Debug)]
pub struct SIMDIter<T : Packable> {
    pub position: usize,
    pub data: Vec<T>,
}

/// A lazy mapping iterator which applies its function to a stream of vectors.
#[derive(Debug)]
pub struct SIMDMap<I, F> where I : SIMDIterator {
    pub iter: I,
    pub func: F,
    pub default: I::Vector,
}

impl<T> SIMDArray for SIMDIter<T> where T : Packable {
    #[inline(always)]
    fn load(&self, offset: usize) -> Self::Vector {
        Self::Vector::load(&self.data, offset)
    }

    #[inline(always)]
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector {
        Self::Vector::load_unchecked(&self.data, offset)
    }

    #[inline(always)]
    fn load_scalar(&self, offset: usize) -> Self::Scalar {
        self.data[offset]
    }

    #[inline(always)]
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar {
        *self.data.get_unchecked(offset)
    }
}

impl<'a, T> SIMDArray for SIMDRefIter<'a, T> where T : 'a + Packable {
    #[inline(always)]
    fn load(&self, offset: usize) -> Self::Vector {
        Self::Vector::load(&self.data, offset)
    }

    #[inline(always)]
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector {
        Self::Vector::load_unchecked(&self.data, offset)
    }

    #[inline(always)]
    fn load_scalar(&self, offset: usize) -> Self::Scalar {
        self.data[offset]
    }

    #[inline(always)]
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar {
        *self.data.get_unchecked(offset)
    }
}

impl<'a, T> SIMDArray for SIMDRefMutIter<'a, T> where T : 'a + Packable {
    #[inline(always)]
    fn load(&self, offset: usize) -> Self::Vector {
        Self::Vector::load(&self.data, offset)
    }

    #[inline(always)]
    unsafe fn load_unchecked(&self, offset: usize) -> Self::Vector {
        Self::Vector::load_unchecked(&self.data, offset)
    }

    #[inline(always)]
    fn load_scalar(&self, offset: usize) -> Self::Scalar {
        self.data[offset]
    }

    #[inline(always)]
    unsafe fn load_scalar_unchecked(&self, offset: usize) -> Self::Scalar {
        *self.data.get_unchecked(offset)
    }
}

impl<'a, T> SIMDArrayMut for SIMDRefMutIter<'a, T> where T : 'a + Packable {
    #[inline(always)]
    fn store(&mut self, value: Self::Vector, offset: usize) {
        value.store(&mut self.data, offset)
    }

    #[inline(always)]
    unsafe fn store_unchecked(&mut self, value: Self::Vector, offset: usize) {
        value.store_unchecked(&mut self.data, offset)
    }

    #[inline(always)]
    fn store_scalar(&mut self, value: Self::Scalar, offset: usize) {
        self.data[offset] = value;
    }

    #[inline(always)]
    unsafe fn store_scalar_unchecked(&mut self, value: Self::Scalar, offset: usize) {
        use std::ptr::write;
        write(self.data[offset..].as_mut_ptr(), value);
    }
}

/// A slice-backed iterator which yields packed elements using the Iterator API.
#[derive(Debug)]
pub struct PackedIter<T : SIMDIterator> {
    pub iter: T
}

/// An iterator which yields multiple elements of a PackedIter
#[derive(Debug)]
pub struct Unroll<'a, T : 'a + SIMDIterator> {
    iter: &'a mut PackedIter<T>,
    amt: usize,
    scratch: [T::Vector; 8]
}

impl<T> PackedIter<T> where T : SIMDIterator, T::Vector : Packed {
    #[inline(always)]
    pub fn unpack(self) -> T {
        self.iter
    }

    #[inline(always)]
    pub fn unroll<'a>(&'a mut self, amt: usize) -> Unroll<'a, T> {
        assert!(amt <= 8);
        Unroll {
            iter: self,
            amt: amt,
            scratch: [T::Vector::default(); 8]
        }
    }
}

impl<T> Iterator for PackedIter<T> where T : SIMDIterator {
    type Item = T::Vector;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next_vector()
    }
}

impl<'a, T> Iterator for Unroll<'a, T> where T : 'a + SIMDIterator {
    type Item = &'a [T::Vector];

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let mut i = 0;
        while i < self.amt {
            if let Some(vec) = self.iter.next() {
                self.scratch[i] = vec;
                i += 1;
            } else {
                break;
            }
        }
        if i > 0 {
            unsafe {
                Some(from_raw_parts((&mut self.scratch).as_mut_ptr(), i))
            }
        } else {
            None
        }
    }
}

impl<'a, T> SIMDRefMutIter<'a, T> where T : Packable {
    #[inline(always)]
    /// Pack and mutate the iterator in place, running `func` once on each packed
    /// vector and storing the results at the same location as the inputs.
    pub fn simd_for_each<F>(&mut self, default: <Self as SIMDIterator>::Vector, mut func: F)
        where F : FnMut(<Self as SIMDIterator>::Vector) -> () {
        let mut lastvec = default;
        let mut offset = 0;

        while let Some(v) = self.next_vector() {
            func(v);
            v.store(self.data, self.position - v.width());
            offset += v.width();
            lastvec = v;
        }

        if let Some((p, n)) = self.next_partial(default) {
            func(p);
            if offset > 0 {
                // We stored a vector in this buffer; overwrite the unused elements
                p.store(self.data, offset - n);
                lastvec.store(self.data, offset - lastvec.width());
            } else {
                // The buffer won't fit one vector; store elementwise
                for i in 0..(self.width() - n) {
                    self.data[offset + i] = p.extract(i + n);
                }
            }
        }
    }
}

impl<'a, T> SIMDRefIter<'a, T> where T : Packable {
    #[inline(always)]
    pub fn simd_map_into<'b, A, B, F>(&'a mut self, into: &'b mut [B], default: <Self as SIMDIterator>::Vector, mut func: F) -> &'b [B]
    where F : FnMut(<Self as SIMDIterator>::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
        debug_assert!(into.len() >= self.scalar_len());

        let even_elements = self.data.len() - (self.data.len() % self.width());
        let mut i = 0;

        while i < even_elements {
            let vec = <Self as SIMDIterator>::Vector::load(self.data, i);
            func(vec).store(into, i);
            i += self.width()
        }

        if even_elements < self.scalar_len() {
            let empty_elements = self.scalar_len() - even_elements;
            let uneven_vec = <Self as SIMDIterator>::Vector::load(self.data, self.scalar_len() - self.width());
            func(default.merge_partitioned(uneven_vec, empty_elements)).store(into, self.scalar_len() - self.width());
        }

        into
    }
}

impl<T> Iterator for SIMDIter<T> where T : Packable {
    type Item = <SIMDIter<T> as SIMDIterator>::Scalar;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data.get(self.position);
        self.position += 1;
        data.map(|d| *d)
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.data.len() - self.position;
        (remaining, Some(remaining))
    }
}

impl<'a, T> Iterator for SIMDRefIter<'a, T> where T : Packable {
    type Item = <SIMDRefIter<'a, T> as SIMDIterator>::Scalar;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data.get(self.position);
        self.position += 1;
        data.map(|d| *d)
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.data.len() - self.position;
        (remaining, Some(remaining))
    }
}

impl<'a, T> Iterator for SIMDRefMutIter<'a, T> where T : Packable {
    type Item = <SIMDRefMutIter<'a, T> as SIMDIterator>::Scalar;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        let data = self.data.get(self.position);
        self.position += 1;
        data.map(|d| *d)
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.data.len() - self.position;
        (remaining, Some(remaining))
    }
}

impl<T> ExactSizeIterator for SIMDIter<T> where T : Packable {

    #[inline(always)]
    fn len(&self) -> usize {
        self.data.len()
    }
}

impl<'a, T> ExactSizeIterator for SIMDRefIter<'a, T> where T : Packable {


    #[inline(always)]
    fn len(&self) -> usize {
        self.data.len()
    }
}

impl<'a, T> ExactSizeIterator for SIMDRefMutIter<'a, T> where T : Packable {

    #[inline(always)]
    fn len(&self) -> usize {
        self.data.len()
    }
}

impl<T> SIMDIterator for SIMDIter<T> where T : Packable {
    type Vector = <T as Packable>::Vector;
    type Scalar = T;

    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        if self.position + self.width() <= self.scalar_len() {
            let ret = unsafe{ Some(Self::Vector::load_unchecked(&self.data, self.position))};
            self.position += Self::Vector::WIDTH;
            ret
        } else {
            None
        }
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<(Self::Vector, usize)> where T : Packable {
        if self.position < self.scalar_len() {
            let mut ret = default.clone();
            let empty_amt = Self::Vector::WIDTH - (self.scalar_len() - self.position);
            // Right-align the partial vector to ensure the load is vectorized
            if (Self::Vector::WIDTH) < self.scalar_len() {
                ret = Self::Vector::load(&self.data, self.scalar_len() - Self::Vector::WIDTH);
                ret = default.merge_partitioned(ret, empty_amt);
            } else {
                for i in empty_amt..Self::Vector::WIDTH {
                    ret = ret.replace(i, self.data[self.position + i - empty_amt].clone());
                }
            }
            self.position = self.scalar_len();
            Some((ret, empty_amt))
        } else {
            None
        }
    }
}

impl<'a, T> SIMDIterator for SIMDRefIter<'a, T> where T : Packable {
    type Vector = <T as Packable>::Vector;
    type Scalar = T;

    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        if self.position + self.width() <= self.scalar_len() {
            let ret = unsafe{ Some(Self::Vector::load_unchecked(self.data, self.position))};
            self.position += Self::Vector::WIDTH;
            ret
        } else {
            None
        }
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<(Self::Vector, usize)> where T : Packable {
        if self.position < self.scalar_len() {
            let mut ret = default.clone();
            let empty_amt = Self::Vector::WIDTH - (self.scalar_len() - self.position);
            // Right-align the partial vector to ensure the load is vectorized
            if (Self::Vector::WIDTH) < self.scalar_len() {
                ret = Self::Vector::load(self.data, self.scalar_len() - Self::Vector::WIDTH);
                ret = default.merge_partitioned(ret, empty_amt);
            } else {
                for i in empty_amt..Self::Vector::WIDTH {
                    ret = ret.replace(i, self.data[self.position + i - empty_amt].clone());
                }
            }
            self.position = self.scalar_len();
            Some((ret, empty_amt))
        } else {
            None
        }
    }
}

impl<'a, T> SIMDIterator for SIMDRefMutIter<'a, T> where T : Packable {
    type Vector = <T as Packable>::Vector;
    type Scalar = T;

    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.data.len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.position
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        if self.position + self.width() <= self.scalar_len() {
            let ret = unsafe{ Some(Self::Vector::load_unchecked(self.data, self.position))};
            self.position += Self::Vector::WIDTH;
            ret
        } else {
            None
        }
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<(Self::Vector, usize)> where T : Packable {
        if self.position < self.scalar_len() {
            let mut ret = default.clone();
            let empty_amt = Self::Vector::WIDTH - (self.scalar_len() - self.position);
            // Right-align the partial vector to ensure the load is vectorized
            if (Self::Vector::WIDTH) < self.scalar_len() {
                ret = Self::Vector::load(self.data, self.scalar_len() - Self::Vector::WIDTH);
                ret = default.merge_partitioned(ret, empty_amt);
            } else {
                for i in empty_amt..Self::Vector::WIDTH {
                    ret = ret.replace(i, self.data[self.position + i - empty_amt].clone());
                }
            }
            self.position = self.scalar_len();
            Some((ret, empty_amt))
        } else {
            None
        }
    }
}

impl<A, B, I, F> Iterator for SIMDMap<I, F>
    where I : SIMDIterator<Scalar = <I as Iterator>::Item>, <I as Iterator>::Item : Packable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Item = B;

    #[inline(always)]
    fn next(&mut self) -> Option<Self::Item> {
        Some((&mut self.func)(I::Vector::splat(self.iter.next()?)).coalesce())
    }

    #[inline(always)]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = (self.len() - self.iter.scalar_position() * self.width()) / self.width();
        (remaining, Some(remaining))
    }
}

impl<'a, I, F> ExactSizeIterator for SIMDMap<I, F>
    where Self : SIMDIterator, I : SIMDIterator {

    #[inline(always)]
    fn len(&self) -> usize {
        self.iter.len()
    }
}

impl<'a, A, B, I, F> SIMDIterator for SIMDMap<I, F>
    where I : SIMDIterator<Scalar = <I as Iterator>::Item>, <I as Iterator>::Item : Packable, F : FnMut(I::Vector) -> A, A : Packed<Scalar = B>, B : Packable {
    type Vector = A;
    type Scalar = B;

    #[inline(always)]
    fn scalar_len(&self) -> usize {
        self.iter.scalar_len()
    }

    #[inline(always)]
    fn scalar_position(&self) -> usize {
        self.iter.scalar_position()
    }

    #[inline(always)]
    fn next_vector(&mut self) -> Option<Self::Vector> {
        self.iter.next_vector().map(&mut self.func)
    }

    #[inline(always)]
    fn next_partial(&mut self, default: Self::Vector) -> Option<(Self::Vector, usize)> {
        let (v, n) = self.iter.next_partial(self.default)?;
        let nr = n * I::Scalar::SIZE / Self::Scalar::SIZE;
        Some((default.merge_partitioned((self.func)(v), nr), nr))
    }
}

/// A trait which can transform a stream of vectors into a contiguous
/// collection of scalars.
pub trait IntoScalar<T> where T : Packable {
    type Scalar : Packable;
    type Vector : Packed<Scalar = Self::Scalar>;

    /// Take an iterator of SIMD vectors, store them in-order in a Vec, and
    /// return the vec.
    #[cfg(not(feature = "no-std"))]
    fn scalar_collect(&mut self) -> Vec<T>;

    /// Take an iterator of SIMD vectors and store them in-order in `fill`.
    fn scalar_fill<'a>(&mut self, fill: &'a mut [T]) -> &'a mut [T];
}

impl<'a, T, I> IntoScalar<T> for I
    where I : SIMDIterator<Scalar = T, Item = T>, I::Vector : Packed<Scalar = T>, T : Packable {
    type Scalar = I::Scalar;
    type Vector = I::Vector;

    #[inline(always)]
    #[cfg(not(feature = "no-std"))]
    fn scalar_collect(&mut self) -> Vec<Self::Scalar> {
        let mut offset = 0;
        let mut lastvec = Self::Vector::default();
        let mut ret = Vec::with_capacity(self.len());

        unsafe {
            ret.set_len(self.len());
            while let Some(vec) = self.next_vector() {
                vec.store(ret.as_mut_slice(), offset);
                offset += Self::Vector::WIDTH;
                lastvec = vec;
            }

            if let Some((p, n)) = self.next_partial(Self::Vector::default()) {
                if offset > 0 {
                    // We stored a vector in this buffer; overwrite the unused elements
                    p.store(&mut ret, offset - n);
                    lastvec.store(&mut ret, offset - Self::Vector::WIDTH);
                } else {
                    // The buffer won't fit one vector; store elementwise
                    for i in 0..(Self::Vector::WIDTH - n) {
                        ret[offset + i] = p.extract(i + n);
                    }
                }
            }
        }
        ret
    }

    #[inline(always)]
    fn scalar_fill<'b>(&mut self, fill: &'b mut [Self::Scalar]) -> &'b mut [Self::Scalar] {
        let mut offset = 0;
        let mut lastvec = Self::Vector::default();

        while let Some(vec) = self.next_vector() {
            vec.store(fill, offset);
            offset += Self::Vector::WIDTH;
            lastvec = vec;
        }

        if let Some((p, n)) = self.next_partial(Self::Vector::default()) {
            if offset > 0 {
                // We stored a vector in this buffer; overwrite the unused elements
                p.store(fill, offset - n);
                lastvec.store(fill, offset - Self::Vector::WIDTH);
            } else {
                // The buffer won't fit one vector; store elementwise
                for i in 0..(Self::Vector::WIDTH - n) {
                    fill[offset + i] = p.extract(i + n);
                }
            }
        }

        fill
    }
}
