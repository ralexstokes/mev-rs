use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rand::prelude::SliceRandom;

fn choose(mut v: Vec<usize>) {
    let mut rng = rand::thread_rng();
    let i = v.choose(&mut rng).expect("at least one element");
    v.remove(v.iter().position(|x| x == i).expect("element not found"));
}

fn shuffle(mut v: Vec<usize>) {
    let mut rng = rand::thread_rng();
    v.shuffle(&mut rng);
    let (_, _) = v.split_first().expect("at least on element");
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("choose", |b| b.iter(|| choose(black_box(vec![2, 5, 3, 5]))));
    c.bench_function("shuffle", |b| b.iter(|| shuffle(black_box(vec![2, 5, 3, 5]))));
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
