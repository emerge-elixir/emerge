# Skia PaintLayer Batching, Parallel Recording, and DDL

Once rendering is structured around explicit PaintLayers, Skia gives us a few ways to make painting cheaper. The important distinction is between batching final composition and parallelizing preparation work.

## Batch final composition

Cached PaintLayers can be represented as GPU-backed surfaces/images and composited in paint order with `drawImage` or `drawImageRect`.

This is the simplest and most important path:

- Render each cacheable PaintLayer into its own surface when it is cold or dirty.
- Reuse the resulting image/surface while the layer stays clean.
- Composite cached layers and freshly painted dirty layers in scene order.
- Flush once for the frame instead of flushing per layer.

This is batching, not parallel drawing. It reduces repeated painting and avoids expensive subtree walks, but final composition still has to preserve order.

## Parallelize recording/preparation

Skia should not be used by drawing into the same live `SkCanvas` or `SkSurface` concurrently from multiple threads.

The safer parallelization point is before final composition:

- Build PaintLayer display lists or pictures independently.
- Prepare cold or dirty PaintLayers off the render thread when practical.
- Keep final replay/composition on the render thread in deterministic order.

For CPU-side recording, `SkPictureRecorder` / `SkPicture` is the simpler model: each worker can record an immutable command stream for a layer, and the render thread can replay it later.

## Deferred Display Lists

DDL means Deferred Display List in Skia.

It is a Ganesh GPU feature for recording draw commands away from the final render surface and replaying them later. In practice, it can be used to parallelize some CPU-side GPU paint recording or preparation work for independent units such as PaintLayers.

How DDL would fit a PaintLayer renderer:

- PaintLayers still define the correctness boundary.
- Each layer can be recorded independently when it is cold, dirty, or missing from cache.
- The final render thread still composites layers in scene order.
- Cached layer images/surfaces remain the simpler and more important first optimization.
- DDL is only a later optimization for the expensive path where a layer must be repainted.

Important constraints:

- DDL does not mean multiple threads draw into the same live `SkCanvas`.
- It does not remove ordering requirements for final composition.
- It mainly helps CPU-side recording/preparation, not the final ordered replay.
- It is Ganesh-specific; newer Skia Graphite uses different recording concepts.
- It adds backend complexity, so it should not be part of the first PaintLayer cache simplification.

## Practical conclusion

First make PaintLayers explicit, independently cached, and composited in order. Once that model is stable, DDL could be evaluated as an optional way to prepare dirty or cold PaintLayers off the render thread.
