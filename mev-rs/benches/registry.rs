use criterion::{black_box, criterion_group, criterion_main, Criterion};

use beacon_api_client::{ValidatorStatus, ValidatorSummary};
use mev_rs::validator_registry::*;

fn extend_summaries_grouped(v: Vec<ValidatorSummary>) {
    let mut state = State::default();
    for summary in v.into_iter() {
        let public_key = summary.validator.public_key.clone();
        state.pubkeys_by_index.insert(summary.index, public_key.clone());
        state.validators.insert(public_key, summary);
    }
}

fn extend_summaries_distributive(v: Vec<ValidatorSummary>) {
    let mut state = State::default();
    let _ = state.extend_summaries(v);
}

fn generate_dummy_summaries(n: usize) -> Vec<ValidatorSummary> {
    (0..n)
        .map(|x| ValidatorSummary {
            index: x,
            validator: Default::default(),
            balance: 0,
            status: ValidatorStatus::Active,
        })
        .collect()
}

fn criterion_benchmark(c: &mut Criterion) {
    c.bench_function("grouped iterator", |b| {
        b.iter(|| extend_summaries_grouped(black_box(generate_dummy_summaries(10))))
    });
    c.bench_function("distributive iterator", |b| {
        b.iter(|| extend_summaries_distributive(black_box(generate_dummy_summaries(10))))
    });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
