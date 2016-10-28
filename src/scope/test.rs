extern crate rand;

use {scope, Scope};
use prelude::*;
use rand::{Rng, SeedableRng, XorShiftRng};
use std::iter::once;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn scope_empty() {
    scope(|_| { });
}

#[test]
fn scope_two() {
    let counter = &AtomicUsize::new(0);
    scope(|s| {
        s.spawn(move |_| { counter.fetch_add(1, Ordering::SeqCst); });
        s.spawn(move |_| { counter.fetch_add(10, Ordering::SeqCst); });
    });

    let v = counter.load(Ordering::SeqCst);
    assert_eq!(v, 11);
}

#[test]
fn scope_divide_and_conquer() {
    let counter_p = &AtomicUsize::new(0);
    scope(|s| s.spawn(move |s| divide_and_conquer(s, counter_p, 1024)));

    let counter_s = &AtomicUsize::new(0);
    divide_and_conquer_seq(&counter_s, 1024);

    let p = counter_p.load(Ordering::SeqCst);
    let s = counter_s.load(Ordering::SeqCst);
    assert_eq!(p, s);
}

fn divide_and_conquer<'scope>(
    scope: &Scope<'scope>, counter: &'scope AtomicUsize, size: usize)
{
    if size > 1 {
        scope.spawn(move |scope| divide_and_conquer(scope, counter, size / 2));
        scope.spawn(move |scope| divide_and_conquer(scope, counter, size / 2));
    } else {
        // count the leaves
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

fn divide_and_conquer_seq(counter: &AtomicUsize, size: usize) {
    if size > 1 {
        divide_and_conquer_seq(counter, size / 2);
        divide_and_conquer_seq(counter, size / 2);
    } else {
        // count the leaves
        counter.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn scope_mix() {
    let counter_p = &AtomicUsize::new(0);
    scope(|s| {
        s.spawn(move |s| {
            divide_and_conquer(s, counter_p, 1024);
        });
        s.spawn(move |_| {
            let a: Vec<i32> = (0..1024).collect();
            let r1 = a.par_iter()
                      .weight_max()
                      .map(|&i| i + 1)
                      .reduce_with(|i, j| i + j);
            let r2 = a.iter()
                      .map(|&i| i + 1)
                      .fold(0, |a,b| a+b);
            assert_eq!(r1.unwrap(), r2);
        });
    });
}

struct Tree<T> {
    value: T,
    children: Vec<Tree<T>>,
}

impl<T> Tree<T> {
    pub fn iter<'s>(&'s self) -> impl Iterator<Item=&'s T> + 's
    {
        once(&self.value)
            .chain(self.children.iter().flat_map(|c| c.iter()))
            .collect::<Vec<_>>() // seems like it shouldn't be needed... but prevents overflow
            .into_iter()
    }

    pub fn update<OP>(&mut self, op: OP)
        where OP: Fn(&mut T) + Sync, T: Send,
    {
        scope(|s| self.update_in_scope(&op, s));
    }

    fn update_in_scope<'scope, OP>(&'scope mut self, op: &'scope OP, scope: &Scope<'scope>)
        where OP: Fn(&mut T) + Sync
    {
        let Tree { ref mut value, ref mut children } = *self;
        scope.spawn(move |scope| {
            for child in children {
                scope.spawn(move |scope| child.update_in_scope(op, scope));
            }
        });

        op(value);
    }
}

fn random_tree(depth: usize) -> Tree<u32> {
    assert!(depth > 0);
    let mut rng = XorShiftRng::from_seed([0, 1, 2, 3]);
    random_tree1(depth, &mut rng)
}

fn random_tree1(depth: usize, rng: &mut XorShiftRng) -> Tree<u32> {
    let children = if depth == 0 {
        vec![]
    } else {
        (0..(rng.next_u32() % 3)) // somewhere between 0 and 3 children at each level
            .map(|_| random_tree1(depth - 1, rng))
            .collect()
    };

    Tree { value: rng.next_u32() % 1_000_000, children: children }
}

#[test]
fn update_tree() {
    let mut tree: Tree<u32> = random_tree(10);
    let values: Vec<u32> = tree.iter().cloned().collect();
    tree.update(|v| *v += 1);
    let new_values: Vec<u32> = tree.iter().cloned().collect();
    assert_eq!(values.len(), new_values.len());
    for (&i, &j) in values.iter().zip(&new_values) {
        assert_eq!(i + 1, j);
    }
}