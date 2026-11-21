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
| **6 of 13** `main` element kinds: `el`, single-line `text`, `none`, `row`, `column`, `wrapped_row`; **one addition: `wrapped_column`** | Padding/gaps, subtree scale, solid backgrounds/text, bundled regular sans/mono. `Input.button/2` builds an `el`; `svg/2` needs the still-missing image kind. |
| Static widths/heights and linear layout | The viewport and El offer both axes independently. A [direct child of El](../test/emerge_skia/el_child_alignment_test.exs) can override the host's alignment on either axis. Row and Column share the main axis; wrapped forms share it **per group**. [Start, center and end groups](../test/emerge_skia/linear_zones_test.exs) use each settled line or column without reordering paint or clicks. Cross-axis alignment uses the padded host or each wrapped group. Static lengths can combine pixels, content and multiple fills. A WrappedColumn can fit all its occupied widths and gaps; an [unproved early dependency](../guides/internals/retained-decisions.md#d10-wrap-columns-at-the-resolved-height) is refused. |
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
