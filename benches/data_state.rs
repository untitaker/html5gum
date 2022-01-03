use html5gum::Tokenizer;
use criterion::{criterion_group, criterion_main, Criterion, BenchmarkId};

fn data_state(c: &mut Criterion) {
    for i in [100, 1000, 10000, 1000000] {
        let s: String = (0..i).map(|_| 'a').collect();
        c.bench_with_input(
            BenchmarkId::new("aaa", i), &s,
            |b, s| b.iter(|| for _ in Tokenizer::new(s).infallible() {})
        );
    }
}

criterion_group!(benches, data_state);
criterion_main!(benches);
