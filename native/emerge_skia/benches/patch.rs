mod support;

use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use emerge_skia::tree::deserialize::decode_tree;
use emerge_skia::tree::patch::{apply_patches, decode_patches};
use std::hint::black_box;
use support::load_fixtures;

fn bench_patch_decode(c: &mut Criterion) {
    for fixture in load_fixtures() {
        let mut group = c.benchmark_group(format!("native/patch_decode/{}", fixture.id));

        for mutation in fixture.patch_names() {
            let patch_bytes = fixture.patch_bytes(mutation).to_vec();
            group.throughput(Throughput::Bytes(patch_bytes.len().max(1) as u64));

            group.bench_function(mutation, |b| {
                b.iter(|| {
                    black_box(decode_patches(black_box(&patch_bytes))).expect("patch decodes")
                });
            });
        }

        group.finish();
    }
}

fn bench_patch_apply(c: &mut Criterion) {
    for fixture in load_fixtures() {
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let node_count = base_tree.len() as u64;
        let mut group = c.benchmark_group(format!("native/patch_apply/{}", fixture.id));
        group.throughput(Throughput::Elements(node_count));

        for mutation in fixture.patch_names() {
            let decoded_patches =
                decode_patches(fixture.patch_bytes(mutation)).expect("fixture patch should decode");

            group.bench_function(mutation, |b| {
                b.iter_batched(
                    || (base_tree.clone(), decoded_patches.clone()),
                    |(mut tree, patches)| {
                        let invalidation =
                            apply_patches(&mut tree, patches).expect("patch applies");
                        black_box((tree.len(), invalidation))
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.finish();
    }
}

fn bench_patch_decode_apply(c: &mut Criterion) {
    for fixture in load_fixtures() {
        let base_tree = decode_tree(&fixture.full_emrg).expect("fixture tree should decode");
        let node_count = base_tree.len() as u64;
        let mut group = c.benchmark_group(format!("native/patch_decode_apply/{}", fixture.id));
        group.throughput(Throughput::Elements(node_count));

        for mutation in fixture.patch_names() {
            let patch_bytes = fixture.patch_bytes(mutation).to_vec();

            group.bench_function(mutation, |b| {
                b.iter_batched(
                    || (base_tree.clone(), patch_bytes.clone()),
                    |(mut tree, bytes)| {
                        let patches = decode_patches(black_box(&bytes)).expect("patch decodes");
                        let invalidation =
                            apply_patches(&mut tree, patches).expect("patch applies");
                        black_box((tree.len(), invalidation))
                    },
                    BatchSize::SmallInput,
                );
            });
        }

        group.finish();
    }
}

criterion_group!(
    benches,
    bench_patch_decode,
    bench_patch_apply,
    bench_patch_decode_apply
);
criterion_main!(benches);
