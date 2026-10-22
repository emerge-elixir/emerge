# WSL/Wayland acceptance follow-up

Issues: [#5](https://github.com/emerge-elixir/emerge/issues/5),
[#6](https://github.com/emerge-elixir/emerge/issues/6),
[#20](https://github.com/emerge-elixir/emerge/issues/20).
Worktree: `/workspace/emerge-wsl-fixes`, branch `fix/wsl-compat`.

Implementation contracts now live in `guides/internals/events.md`,
`lib/emerge/ui/input.ex` and the WSLg section of `lib/emerge.ex`.
Keep only the remaining acceptance/research here, not completed implementation
checklists or validation logs.

## Remaining acceptance

- [ ] Obtain a current-version WSLg repro with Windows/WSL/WSLg versions, distro,
  Emerge revision/source-vs-artifact, build-info output and relevant Wayland/
  Xcursor settings. Capture only a short sanitized trace, not unrelated
  environment variables or secrets.
- [ ] On WSLg and native Wayland, check default/pointer/text cursors with an
  installed theme, broken/missing themes and compositor cursor shapes. Exercise
  normal/HiDPI scales, monitor transitions, enter/leave, pointer capability
  removal/recreation and shutdown. Confirm bounded diagnostics and no invisible
  cursor. An isolated no-cursor-shape compositor fixture should additionally
  exercise SHM release and scale changes without touching host devices.
- [ ] Confirm double-click selection and replacement/copy in single-line and
  wrapped multiline inputs on WSLg. Compare Shift-click, ordinary dragging, IME,
  application echoes and fast repeated clicks with expected UX. Further clicks
  currently keep selecting the word; triple-click line selection is intentionally
  not implemented. Beyond the double-click jitter threshold, dragging extends a
  character range from the word's start.
- [ ] Retest source compilation and precompiled consumers on clean Ubuntu 22.04
  and the reporter's affected Mesa image. Separate download, dynamic loading,
  source linking and window-startup failures. The explicit Wayland/raster route
  avoids GBM, but default shared OpenGL still legitimately includes headless
  PRIME/GBM support. Do not close #5 merely because DRM is optional.

If GBM-free **OpenGL** becomes a requirement, evaluate lazy-loading the narrow
headless PRIME dependency separately. Moving GBM behind `drm` would break the
shared API contract. A new feature instead requires coordinated catalog,
resolver, artifact-identity, checksum and dependency-isolation changes; it is not
part of the current WSL fixes. Do not suggest arbitrary Mesa downgrades or
unversioned library symlinks.

Hardware/compositor access requires explicit permission. No WSLg session was
available during implementation, so unit/CI success must not be represented as
WSLg qualification. Related release gates remain in
[native build rollout](active-build-capabilities.md) and
[Linux qualification](active-linux-gpu-qualification.md).
