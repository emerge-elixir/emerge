# Platform inventory — not animation presentation qualification

| Platform/path | Observed availability | Qualification status |
|---|---|---|
| Native raster | Existing native raster/test harness executes | Functional pixel/scene/hit fixtures pass; not device cadence |
| Linux Wayland | `/wayland-runtime/wayland-1` socket present; compositor v6, DMA-BUF v5, DRM syncobj manager v1 advertised | **Available, not yet qualified** for remaining animation presentation/queue traces |
| Linux Vulkan | RX 7900 XTX and Ryzen integrated GPU enumerate through RADV; Mesa 26.2.2, Vulkan 1.4.354 | Capability enumeration only; not a render/present or headless Vulkan animation pass |
| Linux OpenGL/headless | Existing default-feature build/test coverage; render nodes present | No new selected/drawn/presented GPU animation trace in this slice |
| DRM | `card0`, `card1`, two render nodes present | Device presence is not DRM-master/scanout permission or a display test; do not disrupt the running compositor to claim qualification |
| macOS | No macOS runner/device used | Unavailable here; matching protocol14 host artifact and real presentation run still required |
| Constrained target | No target device or accepted live-memory budget available | Unavailable; desktop timings/RSS cannot substitute |

Raw inventory: `host.txt`, `vulkan-summary.txt`, `wayland-interfaces.txt`.
The enumeration commands were `vulkaninfo --summary` and a bounded `wayland-info`
interface query, not animation workloads. General Linux backend/Vulkan/PRIME
qualification stays owned by `plans/active-linux-gpu-qualification.md`.

P8 still requires backend-selected scene, drawn scene, presentation/overwrite,
final/cleanup frame, queue pressure, interruption/reopen/idle/stop and cadence
reports at an immutable source/build identity. No platform checkbox is closed by
this inventory.
