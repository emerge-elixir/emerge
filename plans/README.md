# Retained renderer: port overview

The target is what an application can do on Emerge `main`
(`aef5e42fc789c396505f85c34810b2ca9c008586`). On the retained renderer an app can
mount a viewport, draw and update a tree, and click what was actually submitted.
It **cannot** yet run the full `main` UI or renderer API. The
[API parity tracker](api-parity.md) lists the remaining work and the one slot for
selected work.

| What works now | Boundary |
| --- | --- |
| `use Emerge`: mount, render, rerender, configure, receive raw input and route clicks; direct renderer startup, status, capture and shutdown | Ordinary binary headless and explicit Wayland sessions. Input uses the **last successfully submitted frame**, not the last accepted tree. |
| **6 of 13** native element kinds: `el`, single-line `text`, `none`, `row`, `column`, `wrapped_row` | Leading layout, padding/gaps, subtree scale, solid backgrounds/text, bundled regular sans/mono. `Input.button/2` builds an `el`; `svg/2` needs the still-missing image kind. |
| Static widths and heights on the six kinds | The viewport and an El offer each axis independently. Row shares width, Column shares height. WrappedRow shares widths **per line**: one fill leaf can be nested under `min`/`max` with pixels and `content()`. Widths combining multiple fills and fill-bearing heights on its direct children remain open. |
| **1 of 19** element `on_*` listeners: `on_click/1` | A left press/release can deliver local coordinates and the current callback for the submitted occurrence. Raw subscriptions are separate; they don't implement key, drag or hover listeners. |
| **3 of 6** binary headless pixel formats: RGBA8888, BGRA8888, RGB888 | CPU-delivered `VideoInterop.Frame` and installed-frame RGB/RGBA or PNG capture. Producing a binary frame is not video import or PRIME export. |
| Wayland CPU (`wl_shm`), OpenGL and Vulkan | Compositor pixels and seat input on all three. Fractional display scale was exercised on shm; hardware GPU evidence is at integer scales, not a synchronization or physical scanout qualification. |

These counts have different denominators. There is no meaningful overall API
percentage: a color constructor, a listener and a display backend are not equal
units of work. The last implementation suite had 93 failures and 21 exclusions;
that is a regression baseline, not a port score.

## How we work

Select **one observable behavior** from the tracker. Record its `main` reference,
expected result and how to observe it. After it is delivered, update the coverage
above, retire that item, and remove its plan. Do not queue several active ports
or keep finished plans here.

Keep the [decision record](../guides/internals/retained-decisions.md) when the
implementation route **or** behavior differs from `main`. Put user-visible deltas
in [changelog candidates](../guides/internals/retained-changelog-candidates.md),
including intentional changes as such. Those two records outlive any plan.
