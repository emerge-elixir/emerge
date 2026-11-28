# Where the memory goes: 20,000 Field Notes before virtualization

**The live renderer allocates no layout or paint workspace for this upload.** It rejects the declaration at revision 0. On the real publisher path the writer nevertheless reached **233.71 MiB** of outstanding Rust allocation requests. In a separate experiment, reserving the workspace **one storage family at a time** accounted for **5.91 GiB**. Of that, **4.48 GiB (75.8%) is immutable drawing, hit and indexing pools**. These three numbers describe different things. There is no installed 20k frame or steady-state RAM reading.

The input is the actual `WaylandCards.render/1` Field Notes page, not a synthetic row: 70,034 elements, 444,233 declared glyph slots, an outer page, a 176 px note port, 5,000 flag badges, the pin and footer, no raster images. Only the note count in a private gallery copy changed. The entire EMRG is **4,521,235 bytes (4.31 MiB)**. All measurements use the original declaration on Linux x86-64. Layout and frame samples use a 584 × 506 target at scale 1; the cold reservation is independent of that viewport and funds five live reader slots. Default headroom is 80%. MiB and GiB are binary units.

## 1. What the failed upload actually did

An ordinary binary-headless NIF returned `{:error, "live transaction exceeds storage limit"}` to `Native.renderer_upload`; the wrapper raised that reason. Nothing was queued or installed. The 256-note control (930 elements) reached revision 1 and delivered a frame.

| Ordinary BEAM process measurement, 20k / 80% | MiB (two fresh processes per arm) |
| --- | ---: |
| Full `EmergeSkia.upload_tree`, whole-process peak through failure | 605.53 and 645.23 |
| File-backed test input read into a binary, **no Elixir tree or encoder**: RSS with wire ready → after the same native refusal | 112.16 → 171.25; 116.55 → 175.65 |
| Separate staged run, RSS: renderer started → tree built → wire encoded → native call returned | 97.68 → 251.12 → 586.71 → 648.25; 98.95 → 248.45 → 579.51 → 642.63 |

In the staged run most process RAM was already present **before submission**. The ready-wire arm added about **59 MiB of RSS** across the call. These are within-process checkpoints, not additive estimates from different processes. The staged arm kept the candidate state and assigned tree available across submission; allocator reuse, BEAM collection, and sampling affect RSS. For example, at `encoded`, `:erlang.memory(:total)` was about 141 MiB, not the 580–587 MiB OS reading; subtracting the two does not isolate renderer allocations. Earlier minimally sampled full-wrapper peaks were 604.64 and 604.95 MiB. None is a measurement of drawing 20,000 notes.

A private standalone release executable ran the same streaming, validation, capacity check and rollback on the same bytes. Its allocator counted requested sizes on the calling Rust thread (the input file was already loaded). At default headroom:

| Publisher checkpoint | Outstanding requested MiB | What exists |
| --- | ---: | --- |
| After feed | 8.99 | Staged rows, byte ranges and stream storage |
| After validation | 127.71 | Final-topology proof for 70,034 records, including the edit batch |
| Just before the workspace check | 233.71 | Also capacity for writer mount keys, IDs and spare indices |
| After refusal, before dropping the publisher | 224.72 | Logical edits cleared, reusable backing still owned |
| After dropping the publisher | 0.00 | All tracked outstanding bytes returned; not a claim that libc returned pages to the OS |

Feed plus prepare requested **245,517,207 B cumulatively** (a realloc counts its new size); peak outstanding was **245,064,451 B**. At the error, **106.00 MiB** was in writer mount/index storage and **118.73 MiB** in proof storage. The two largest allocations were the empty-capacity mount array (102.91 MiB) and the edit-record array (86.56 MiB). Only **0.14 MiB** of the writer mount/index ranges and **54.48–56.12 MiB** of the proof ranges were on present pages then. The transient document-credit peak was 12,929,411 B and returned to zero. Both current **and peak workspace grants were zero**. No Context, Measure, glyph shaping, Project, or raster work ran for this submission.

Setting headroom to zero reduced the native writer's peak outstanding to **135.35 MiB**, but feed plus prepare then requested **529.08 MiB cumulatively**: validation alone made 15,013 reallocations instead of 49. Less capacity held is not the same as less allocation traffic.

## 2. Measured storage by retained layer

To measure the layers rather than infer them from a grant, the diagnostic first required the real refusal, dropped the writer, then populated an **offline** fixture from the same bytes. It ran the production layout scheduler through Compose, stopping just before Project. It used the actual default workspace dimensions: 70,034 live elements → **126,062 owner slots and 226,912 key slots** (the already-grown key count receives headroom again). The run resolved **all 70,034 frames** and a vertical note-port range of **879,820 px**. The 256-note offline control matched the accepted path's geometry digest and 11,084 px range.

**Held** is the requested heap backing attributable to a store when destroyed, including slot-map metadata, initialized empty slots and spare capacity. **Present** is the portion of those same requested byte ranges on present anonymous pages in each of two snapshots after Compose, before inspection or teardown. It is not `sizeof(value) × occupied`, an allocation made *during* the pass, or per-layer process RSS. A page can contain empty capacity or allocator reuse. Each shared store is charged **once** below; queues and output pools are outside this table.

| Owner | Direct MiB | Shared/helper MiB | Held MiB | Present MiB | What the storage holds |
| --- | ---: | ---: | ---: | ---: | --- |
| Context | 15.58 | — | **15.58** | 3.06–3.51 | Inherited layout inputs; early height shares (no shares on this page) |
| Measure | 34.62 | 122.60 | **157.22** | 80.73–83.34 | Local/parent-facing demands, cross/height shares and 25,013 per-owner run descriptors; helpers below |
| Flow | 10.39 | — | **10.39** | 4.75–5.58 | Width and incoming offer, 70,034 entries each |
| Reflow | 6.92 | 25.31 | **32.23** | 9.60–12.94 | Fitted sizes; shared paragraph solutions, sites, fragments and float scratch |
| Place | 12.98 | — | **12.98** | 5.89–7.60 | Placement and local/parent-facing rectangles; 65,033 placements, 70,034 of each rectangle |
| PaintContext | 39.82 | — | **39.82** | 12.97–13.42 | Per-owner paint geometry, inherited style/clip/scroll context; 70,034 entries |
| Compose | 88.29 | 31.16 | **119.45** | 37.44–38.54 | 70,034 compositions and 20,006 scroll-reach entries used to select descendants |
| Project | 185.24 | — | **185.24** | 58.52–58.97 | Key-indexed optional projection and two saved local-program slots; **not** their payload pools |
| **Eight owners together** | **393.85** | **179.07** | **572.92** | **214.90–221.97** | 600,748,976 B held; per-run sums, not sums of independent minima/maxima |

**Project is largest by capacity; Measure is largest on present pages.** Project, Measure and Compose account for 80.6% of the layer backing. Three details explain why counts of visible rows or glyphs alone don't predict these sizes:

- **Project:** `ProjectionSlot` is 848 B **before** slot-map overhead. It contains an optional projection and two inline `Local` values. PaintContext initializes a placeholder when a key enters scroll content. **70,023** placeholders were occupied before Project, with **zero** actual projections or saved programs. In the isolated first-view sample below there were **40** populated projections and **38** keys with a saved program on each side. The backing was still **194,237,528 B**. A slot is not a recording.
- **Measure:** the 122.60 MiB of helpers is **53.38 MiB** of shared intrinsic-size memoization, **48.45 MiB** of shared shaped text/string storage and indexes, and **20.78 MiB** for independent constrained-fit inputs/results/marks. The last three constrained maps contained **no entries** on this page. The intrinsic table initializes 252,125 optional entries: **38.47 MiB present** for that array alone, only **5,743 occupied**. Text had **20,014 shared runs** for the 25,013 descriptors and **424,237 used glyph records of 1,599,240 reserved**; the glyph data array alone held 18.30 MiB. Paragraphs, composition and projection read these results; they do not each get charged another text store.
- **Reflow and Compose:** three paragraph solutions and 47 settled fragments occupied pools whose maps, hole lists and compaction/float workspace were sized for much larger populations. In contrast, Compose really held a 400 B `Composition` value for every element. Its separate reach index is counted with Compose, not again with Project. The two runtime port states are counted separately below.

Including the eight worksets, deferred-rank queue, payload-bound index, port states and prepared font-strike tables brings the **settled `Layers` owner** to **612.35 MiB held**, **242.37–245.52 MiB on present pages** (unrounded totals in the logs). That excludes declarations, the output compiler, the reader banks and process overhead. In the offline process as a whole, RSS at that checkpoint was 381.00–382.54 MiB; its fixture has a different declaration representation and no installed frame. At **0% headroom** the eight pass-owned totals, in the same order, were **4.81 / 73.67 / 3.21 / 12.56 / 4.01 / 12.29 / 36.87 / 57.17 MiB**. Geometry and entry counts did not change.

## 3. Where the would-be workspace goes

The diagnostic reserved and dropped each family with the production capacities; immutable stocks were built and destroyed **one stock at a time**. No large combined grant was issued. These are measured outstanding **Rust-requested bytes** from cold stores, not credits, process RSS, simultaneous use, or bytes emitted into a frame.

| Disjoint cold workspace owners, 20k / 80% | MiB |
| --- | ---: |
| Immutable recording stocks: draw/hit pages, headers, orders, bindings, materials | **4,591.88** |
| Eight pass-owned stores above, including their helpers | 572.92 |
| Flat drawing / hit buffers: builder plus five readers | 349.03 + 97.85 |
| Compute mounts, ID index, touched keys and empty declaration handles | 190.45 |
| Compute edit/validation batch, distinct from the writer's batch | 118.34 |
| Local recording compiler scratch, binding scratch and material-class headers | 72.98 |
| Eight worksets and deferred ranks | 31.20 |
| Packing stacks, bands and material index | 22.06 |
| Payload-bound index | 8.00 |
| Port states, empty resource handles and boxed workspace/grant overhead | 0.02 |
| **Total, summed before rounding** | **6,054.72 MiB = 6,348,832,737 B (5.91 GiB)** |

**The recording pools dominate before any scrolling.** The current `Stock::reserve` constructs each reference-counted object; small typed leaves also own a backing vector. This is different from leaving one large virtual address range untouched. Across the separate cold reserves, **23,018,467 stock units** were prepared, none issued. Their requested ranges were on present pages in the individual snapshots. This does **not** mean a live process used 4.48 GiB of RAM. The following is a breakdown of the first row above, not additional bytes:

| Immutable stock family, largest first | MiB |
| --- | ---: |
| Drawing-command leaves and branches | 993.53 |
| Hit leaves and branches | 594.54 |
| Child-order (ordinal) nodes | 569.38 |
| Standalone local programs | 507.82 |
| Glyph IDs and positions | 446.17 |
| Drawing and hit program headers | 403.95 |
| Binding edges and windows | 346.31 |
| Clip and transform-plane pages | 313.08 |
| Material versions and size-class byte buffers | 214.16 |
| References to shared materials | 149.07 |
| Visibility ranges | 53.87 |
| Fitted-image versions | 0.00 |

The policy funds both draw and hit output, mutable/replacement versions and up to five independently held readers. Two ports or any escaping child makes every owner a possible standalone local; this page has both. The separate flat buffers also reserve against the full declaration. These are capacities for what *could* be required, not six copies of the visible drawing.

**How much did the first view use?** In an additional offline sample, the 20k settled layout projected and sealed the top view using the smaller, funded **256-note output bank**. It issued **324 stock units** representing **92,600 B (90.43 KiB)** of object/leaf backing. This includes 38 drawing headers, 17 hit headers, 48 command leaves, 19 glyph-ID leaves and 69 child-order nodes. The logical output was 82 commands, 389 glyphs and 18 hits. Issued means a slot was lent out, not newly allocated; this count excludes the free bank, flat buffers, per-key tables, framebuffer and allocator overhead. The 256 control issued 312 units / 89,144 B and its complete raster matched the accepted control. The 20k sample's full raster and geometry digest repeated at both headrooms. **This is one view, not an accepted 20k session, a bound for every scroll position, or a demonstration of virtualization.** On the earlier *admitted* 256-note sweep, drawing headers still issued increased from 38 to 917 after returning to the top.

## 4. Controls and interpretation

| Notes / headroom | Recording credits | Other workspace credits | Combined credits | Measured cold store backing | Live result |
| --- | ---: | ---: | ---: | ---: | --- |
| 256 / 80% | 73,798,274 B | 29,614,958 B | 103,413,232 B | 85,739,947 B | Accepted |
| 256 / 0% | 40,957,147 B | 12,294,976 B | 53,252,123 B | 45,233,967 B | Accepted |
| 20,000 / 80% | 5,477,689,241 B | 2,179,118,224 B | **7,656,807,465 B** | **6,348,832,737 B**, piecewise | Refused |
| 20,000 / 0% | 3,039,728,491 B | 895,987,856 B | **3,935,716,347 B** | **3,350,143,983 B**, piecewise | Refused |

On **both admitted controls** the disjoint cold ownership sum equalled the bytes released by dropping the **complete** production workspace, **exactly**. At 256 / 80%, the full cold call requested 86,807,219 B cumulatively and released 1,067,272 B during construction, leaving 85,739,947 B. The release executable excludes `#[cfg(test)]` storage. As cross-checks at 20k / 0%, the 59,949,960 B mount array, 28,222,608 B text store and 71,164,260 B range/projection/Project-workset group match the earlier isolated probes. The test build additionally had an 8,964,352 B trace and a 24 B test-only paragraph-solution field per addressable slot. The earlier *single-key-pad* and *domains-only batch* trials are alternatives, not baseline savings to subtract here.

At 20k / 80%, recording alone exceeds its 1 GiB calculation limit. **Even without a single recording, the other 2.03 GiB of credits exceeds the 512 MiB single-workspace-grant limit.** So evicting visited rows alone would miss both the initial recording reservation and a second, independent limit. Conversely, making the EMRG smaller would not make the per-key slots or immutable banks smaller. No cache policy, quotas, reader count or app declaration were changed for this profile.

## Reproduction and boundaries

- Production sources: [workspace sizing and proof](../../../native/emerge_skia/src/tree/wire/publisher.rs), [retained layers](../../../native/emerge_skia/src/tree/layers.rs), [intrinsic cache](../../../native/emerge_skia/src/tree/intrinsic.rs), [projection slots](../../../native/emerge_skia/src/tree/paint.rs), [stock construction](../../../native/emerge_skia/src/tree/paint/pages.rs), [recording credits](../../../native/emerge_skia/src/tree/paint/recording/admission.rs). The [residency proposal](../../scroll-recording-residency.md) is separate from these results.
- Evidence directory: `/workspace/.notes-window-Xbt5vu/`. `samples2/` contains the release executable, exact source overlay, build manifest, build log and source ID `ea493cc47f0d2d1b0204b40380fa5d2b16f41bed5cc46d94f88ced21a29ccb45`. `samples2-logs/` and `repeat/` contain the 32 native runs; `beam2/` contains the 12 ordinary NIF runs. `tables2/` contains exact disjoint bytes, entry counts, capacities, page observations and first-view issuance (not rounded MiB). `tables2.log` contains the control comparisons. The immutable 20k fixture is `/workspace/.notes-20k-nShPBY/live-2/20000.emrg`, SHA-256 `dba73ecfac8211a1ad73a6791b6e57fb319d2f070b92a2bd653beb8238d0b9ae`.
- From the evidence directory on this host: `bash run samples2 NEW_LOG_DIR` repeats both note counts × both headrooms × `ingress`, `cold`, `layout`, `sample`; `bash beam-check NEW_BEAM_DIR` repeats the ordinary-NIF checkpoints. `bash build NEW_BUILD_DIR` rebuilds the private executable with locked/offline dependencies and a source-revision lock. No allocator interposition in BEAM. The stock issuance and `/proc/self/pagemap` observers exist only in the standalone executable. Byte counts use `System` allocator layouts; snapshots intersect each allocation once with anonymous present pages frozen before store inspection, not before/after process RSS differences. Calibrations exercise alloc/zeroed/realloc/free and touched versus untouched pages.
- Frame digests are of complete 584 × 506 RGBA rasters. The diagnostic has no presenter or input route. No claim about installed 20k memory, maximum visible demand, images on other pages, GPU memory, first-frame latency, FPS, eviction or steady-state scrolling follows from it.
- Independent source-matched checks (`ordinary2/`): default release `cargo test` 364 passed; `mix test` for admission and scrolling 14 passed. Full seed-17 Mix remained at the known 39 failures and 21 exclusions, with no names added or removed. No production code or test fixture changed.
