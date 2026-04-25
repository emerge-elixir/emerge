mod support;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::serialize::encode_tree;
use std::hint::black_box;
use support::load_fixtures;

fn bench_emrg(c: &mut Criterion) {
    for fixture in load_fixtures() {
        let decoded_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let mut group = c.benchmark_group(format!("native/emrg/{}", fixture.id));
        group.throughput(Throughput::Bytes(fixture.full_emrg.len() as u64));

        group.bench_function("decode_only", |b| {
            b.iter(|| black_box(decode_tree(black_box(&fixture.full_emrg))).expect("EMRG decodes"));
        });

        group.bench_function("encode_only", |b| {
            b.iter(|| black_box(encode_tree(black_box(&decoded_tree))));
        });

        group.bench_function("decode_encode", |b| {
            b.iter(|| {
                let tree = decode_tree(black_box(&fixture.full_emrg)).expect("EMRG decodes");
                black_box(encode_tree(&tree))
            });
        });

        group.finish();
    }
}

criterion_group!(benches, bench_emrg);
criterion_main!(benches);
