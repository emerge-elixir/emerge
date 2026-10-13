# Embedded artifact follow-up

## Remaining checks outside Trellis

- Verify released x86_64 musl and RISC-V64 packages in fresh Nerves consumers
  without Rustler, including `nerves_system_x86_64 1.34.2` and
  `nerves_system_mangopi_mq_pro 0.17.2` (or record replacement system versions).
  Exercise download, cache, integrity failure, and explicit source fallback.
- Confirm BEAM/NIF loading and rendering on those target systems. Host/QEMU
  load checks do not establish physical-board graphics/driver compatibility.

Do not retain completed cross-build logs as an ARMv7 qualification blocker.
Release archive/checksum validation is already owned by the workflows and
[the native build guide](../guides/reference/native-renderer-builds.md).
CPU-only DRM cadence/fault work on Nexus 7 is tracked in
[Linux platform follow-up](active-linux-gpu-qualification.md).
