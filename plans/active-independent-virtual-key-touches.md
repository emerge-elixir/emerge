# Independent virtual-key touches

## Status

Implemented in the Emerge worktree, without changing app keyboard attributes.
The mouse tracker remains separate; DRM contact IDs, captures, hit regions,
pressed-feedback ownership and hold/repeat deadlines are contact-local. Release
activates one armed tap; cancel/slide-off/remount/blur/input-disable cannot activate.
Ordinary UI still uses one primary contact through the existing pointer pipeline.

Regression coverage includes overlapping release orders, same-key contacts,
independent holds/repeats, mixed mouse/touch ownership, focus/keyboard replacement,
stale FIFO/observer-once behavior and ordinary row tap/scroll/cancellation. Native
and focused Elixir tests pass. Normal Mix tests are environment-blocked; full FP3
firmware, physical typing and daemonless hotplug validation remain pending.
See the [bundling plan](active-drm-libinput.md) for build/link checks and limitations.

## Original problem

DRM collapsed all touchscreen slots into one cursor and BTN_TOUCH. The event
runtime also shared one mouse virtual-key tracker. A second finger could therefore
move/cancel the first key, and no independent release reached it.

## Scope

- Preserve contact identity and down/move/up/cancel phases for DRM direct touch
  via the [self-contained libinput migration](active-drm-libinput.md), rather than
  a new evdev slot decoder. Bundle its libudev layer too; do not assume a system
  libudev, udev daemon or modified nerves_system. The first evdev-specific draft
  is archived outside the worktree.
- Track virtual-key presses, cancellation and hold/repeat deadlines per contact.
- Keep tap activation on release; only that contact's movement can cancel it.
- Preserve existing mouse handling and single-primary-touch handling for other UI.
- Do not introduce multi-touch scroll/gesture listeners or change keyboard attrs.
- Cancel contacts on input loss, device removal, blur, key removal or remount.

## Validation

- Validate bundled input against an unmodified Nerves SDK and a runtime without
  libudev, a udev daemon, system quirks data or a hotplug helper.
- Translate overlapping libinput contacts, cancellation and slot reuse without
  cross-talk, including touch-frame boundaries.
- Type overlapping keys in either release order, including buffered input/rebuilds.
- Check independent pressed feedback, same-key contacts, slide-off cancellation,
  hold/repeat timers, blur/remount cleanup and ordinary touch scrolling/taps.
- Run native tests and attempt Mix tests; document environment blockers.
