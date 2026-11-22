# Producing a v10 patch from a complete declaration

Comparison: retained `ffa22073c663f61b7dda59c11f0ca8af67422abc` against the Elixir snapshot producer in this change. No viewport data-source API, native code, wire format or heap-policy change. A key still identifies an occurrence, not an unchanged subtree.

**One label in 500 rows:** the assigned producer went from **980 to 644 µs** (paired **−34.3%**, 95% interval −34.9% to −33.7%; paired arithmetic difference −336 µs [−342, −330]). It still visited all **1,002** nodes and emitted the same **113/105-byte** forward/undo patches. Traced words per edit fell from **277,898 to 171,908** (−38.1%); binary-only fell from **263,873 to 158,883** (−39.8%). These are improvements to reconciliation, not new layout timings.

## What changed

- Match against the **old** occurrence index. For a generated keyed occurrence, the new declaration's key→ID entry is already its claim. Check the ID as well as the key when retiring. Unkeyed matches and all detached imports still track ID claims. The seen keys become the accepted key index; reuse the old map if every entry (and the count) agrees.
- Visit children in declaration order and build their output on return. No intermediate reversed list of child folds. If a forward reference is pending, settle it against the complete new key index, still in declaration order. A stable VNode suffix is shared; a changed prefix is copied once. Assigned output keeps current `Element.frame` and other public fields. Binary and routed updates produce no assigned tree.
- Reuse a VNode only after its exact resolved attrs, owner, inherited/site inputs and children agree. This includes callback addresses, even if the native patch is empty. Collect changed node records, install them in the index together, then retire old unclaimed occurrences. Stable records are not reindexed.
- Keep descriptor comparisons exact. An inherited-font edit really rebinds **1,300** descendant sites; only **one** ancestry bind is unchanged. Queue reference decrements until all new claims have been made rather than writing zero-count descriptors only to delete them. A descriptor reclaimed within the update keeps its token. A cold list mount also encoded **1,001 empty maps** for inherited inputs: the attr codec now emits their existing two-byte zero count directly. Nonempty maps follow the same validation/encoding path.

## How to interpret the timings

Native first/changed/clean refresh is **not** a producer measurement. The matched native code and patch bytes are unchanged. In the [native baseline](shared-surface-baseline.md#native-compute) the tall list took **935.5 µs** for first layout after upload, **29.0 µs** for a label patch and **0.032 µs** for unchanged reuse. The grid took **672.5 / 53.5 / 0.032 µs** for first / inherited font / clean. Those are earlier measurements, not new speedups. The unchanged native adapter was replayed on the new producer's bytes.

The timing runs use eight alternating fresh VM pairs, CPU 8, one Rayon worker, one BEAM scheduler, normal caller heaps and the exclusive performance lock. Every VM checked the loaded BEAMs and mapped NIF. Declarations were prepared before the producer timer. Times are medians of the eight process means; changes and approximate unadjusted 95% intervals are paired log ratios (df=7). Independently timed producer, native and frame measurements are not additive. GC tracing, `:tprof` words and `:erts_debug.size/1` are separate diagnostics.

### Untraced producer and actual viewport

| Prebuilt declaration | Assigned, µs before → after | Binary-only, µs before → after | Routed, µs before → after |
| --- | ---: | ---: | ---: |
| Cold list mount/encode from empty state | 4,487 → 3,425 | 4,123 → 4,011 | 4,014 → 3,983 |
| Rebuilt equal list | 943 → 583 | 840 → 537 | 838 → 537 |
| One list label | 980 → 644 | 876 → 597 | 880 → 593 |
| Callback payload only | 924 → 583 | 824 → 548 | 827 → 539 |
| Add/remove a listener | 986 → 652 | 916 → 588 | 879 → 588 |
| Reverse 500 keys | 1,989 → 1,569 | 1,865 → 1,484 | 1,882 → 1,484 |
| Insert/remove a row | 1,840 → 1,587 | 1,762 → 1,449 | 1,764 → 1,436 |
| Inherited font on 100 cards | 2,631 → 2,041 | 2,457 → 1,903 | 2,475 → 1,891 |

The upper time limits for all 24 producer comparisons are below the proposed +5% margin. For the cold encode the upper limits are −22.3%, +2.2%, +0.3%; for inherited font all three are below −21%. The actual application message → viewport rerender → verified list frame went **2.034 → 1.666 ms**, paired **−18.2% [−25.7%, −9.9%]**. Those frames are 960×640, 2,457,600 bytes each. A public patch returning an assigned tree and a real viewport rerender are not the same caller.

In the separate one-label phase harness the pre-encoded control was **1.059 → 1.057 ms** (+3.0% [−4.4%, +11.0%]), the public assigned call → frame **2.487 → 2.603 ms** (−5.6% [−20.1%, +11.4%]), and the standalone runtime call → frame **1.827 → 2.385 ms** (+12.9% [−9.8%, +41.3%]). None establishes a +5% non-regression bound for those delivery endpoints. The frame can be in the mailbox before the caller resumes.

The unchanged shared-surface benchmark provides a second caller. All **12/12** producer cases improved, including the actual `Engine.encode_full/2` call: list **4.005 → 3.241 ms** (−18.7% [−20.2%, −17.3%]); grid **4.271 → 3.426 ms** (−20.1% [−20.8%, −19.3%]). **8/9** public call → frame cases improved, including list label **3.330 → 1.812 ms** and inherited font **4.442 → 3.655 ms**. **One regressed:** changing card fill weights took **2.291 → 2.750 ms** (+20.4% [+17.3%, +23.6%]) although the binary producer went **981 → 622 µs**. The producer timing cannot explain the frame by subtraction. In additional **actual viewport** runs, inherited font went **3.443 → 2.964 ms** (−15.9% [−19.6%, −11.9%]) and fill weights **1.885 → 1.783 ms** (−12.2% [−17.2%, −7.0%]). The standalone public fill-weight regression is real in that caller; it did not carry over to the viewport. Do not generalize the label win to all delivered frames.

### Allocation and trial decisions

The original `:tprof` workload is 12 alternating edits per mode. These are words attributed to traced calls, not bytes on the wire, RSS or exclusive function timings.

| Trial | Label words/edit, assigned / binary | Decision |
| --- | ---: | --- |
| `base` | 277,898 / 263,873 | Frozen retained control; the earlier [label investigation](list-label-delivery.md) is not the comparison arm. |
| `claim` | 249,664 / 235,639 | Keep exact key→ID claims. Isolated assigned time −7.2% against base. |
| `stable` | 181,700 / 167,675 | Keep stable VNode return after settlement. Assigned time −19.2% against `claim`. |
| `children` | 181,174 / 168,149 | Keep suffix sharing. Assigned time −6.9% against `stable`; binary words alone did not improve at this step. |
| `keys` → `fold` | 182,197 / 169,172 → 171,185 / 158,160 | Keep the final key map and eliminate the intermediate fold list. Neither keys nor a branch stamp bypass descendant checks. |
| `share` → `batch` | 172,153 / 159,128 → 172,161 / 159,136 | Share exact attrs with assigned output; collect only changed VNodes before merging the index. The benefit of the batch was on inherited font: 721,254 → 683,739 traced assigned words, not on the label. |
| `labels` → `ready` | 171,921 / 158,896 → 171,908 / 158,883 | Defer descriptor decrements; inherited-font words fell again to 607,817 (base: 777,955). Keep the empty-map encoder: cold assigned encode was −18.2% [−20.4%, −16.0%] against `labels`; the label time was inconclusive. |

The intermediate `candidate`/`direct` comparison rejected a nested `put_in` on every changed node. `table` built the node map in bulk on vacant mounts only; `batch` supersedes it. `axes` short-circuited empty scroll axes but missed the cold-encode guard; it is not in the result. `empty` and `combined` tested empty encoding *before* index changes: even when the isolated cold update passed, the original list full-encode caller could regress (**+7.3%** for `combined`). `mount` also tried omitting generated unkeyed ID claims on a vacant mount; the final producer needs no exception. Raw immediate-parent and cumulative comparisons are retained; improvements are not additive.

`DiffState` reachable words on the label edit: assigned **138,274 → 117,309** (−15.2%); binary **102,782 → 102,779**. Stable VNodes do not keep old assigned Element trees. The update temporarily holds changed node records and pending reference decrements, not a second retained index. After each of 20 churn cycles both arms had six keys, nine nodes, three routes and 3/5/8 own/branch/ancestry descriptors. Clear left all live counts zero, 72 reachable words and the next ID at 16,250.

In the separate **GC-traced** normal-heap caller, arithmetic mean assigned label time was **1.724 → 1.394 ms**. Caller-GC overlap was **0.681 → 0.622 ms** (paired difference −0.059 ms [−0.081, −0.037]); time outside GC **1.043 → 0.772 ms** (−0.271 [−0.288, −0.253]). With the diagnostic large minimum heap there was no GC overlap in either producer, and means were 0.869 → 0.586 ms. This is not an attribution of an earlier session's latency or a reason to change viewport heaps.

## Correctness and scaling controls

- **768** saved transitions from the frozen producer: assigned, binary, routed and alternating-mode chains; fixed and seeded edit/undo histories. Compared the bytes, public fields, IDs/next ID, full VNode/index including stamp pools, registry and routed delta. The same serialized declarations preserve callback roles across VMs. New tests also exercise refusals, detached duplicate IDs, key/kind replacement, multiple pending bindings and independent sparse expansion.
- On supported list and grid scenes, checked forward/undo bytes, declaration-ordered geometry and **20 complete fresh frames** across 960×640 and 960×4000; visible sparse edits and undos matched fresh. The actual native adapter matched all **24** tall states, including the listener-only target. Headless press/release and routed local coordinates across a held reorder, and sealed vs installed route publication, are separate tests. No window or scanout claim.
- Edit the last label in 50 / 500 / 5,000 keyed rows. Both producers enter **101 / 1,001 / 10,001** nodes for a 51-byte patch. There are `2n` ordinary edges: the new producer visits each once on entry and folds each once on return, without constructing a second fold list. Both stamp `n+1` child edges. Base wraps `2n` entries for candidate VNode lists before reversing; the new producer constructs `n+1` cons cells (including the wide prefix). Assigned mode also constructs `2n` assigned edges; binary constructs none. This is still **linear**, including constructing an independently rebuilt declaration. Separate untraced construction took 45 / 495 / 4,962 µs on the candidate (base: 45 / 481 / 4,881); none of those paired intervals establishes a change. The 1,002-node delivery fixture has an additional outer wrapper.
- Extra producer controls exercised 65,535 siblings and a forward reference after 10,000 rows. The base and candidate emitted identical patch and state signatures on mount, edit, undo and clear.

**Qualification.** On the selected source, `cargo test` passed **173/173**. The focused identity, diff-state, submit-route and headless interaction tests passed **59/59**. Adding patch, reconcile and controlled-scroll tests gave **84/94**; the ten failures are all in the existing failure list. Full `mix test`: **772/865 passed**, with exactly the baseline's **93 named failures and 21 named exclusions** (baseline: 766/859; six tests added). Changed Elixir files passed `mix format --check-formatted`, strict Credo and compilation with warnings as errors. The snapshot allocation and time target is met without growing live state; the standalone delivery exception above is not a renderer improvement.

## Replay

Use a new copy of `/workspace/emerge-investigation-artifacts/snapshot-diff-4yC1Kn/`; start with `HOWTO.md` and `evidence.sha256`. `base` is `ffa2207`, `ready` is the selected source. Its `logs/ready.source` manifest has SHA-256 `e66e9aaf973374eda015eeb09f84aefe78687b8ee7a7b0ed1580024ae50b214e`; mapped release NIF `935594e5fa59efdc252204b56d9f9b5d777b202500a5eeea0ca667fb141f9cff` on both sides. `raw/ready/inputs.sha256` seals the producer/viewport comparison, `raw/extra-ready/` has the original caller plus GC, construction and live-state controls, and `raw/grid-ready/` has the additional grid viewports. `oracles/` holds the frozen transition results, `captures/` the whole-frame and geometry checks, `logs/report-*` the estimates and intervals including rejected trials. The [scope and gates](../../reconciliation-and-diffing.md) still exclude a viewport dependency system. Nothing here changes native API coverage.

Evidence seal: **29,317** entries; SHA-256 of `evidence.sha256`: `cbed424a8bdcdd58f4256142d649e51eeda93c31626abdb312cf6c288bfddee1`. The bundle contains this checkpoint before adding its own seal digest.
