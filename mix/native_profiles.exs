# Shared data only: compiled into BuildProfiles; also used by standalone release tooling.
# System capabilities describe exact stock image configurations, not SoC limits.
%{
  embedded: [
    "armv7-unknown-linux-gnueabihf",
    "x86_64-unknown-linux-musl",
    "riscv64gc-unknown-linux-gnu"
  ],
  darwin: ["aarch64-apple-darwin", "x86_64-apple-darwin"],
  systems: [
    %{
      version: "2.0.1",
      app: "nerves_system_rpi5",
      board: "Raspberry Pi 5",
      capabilities: %{atomic_kms: true, opengl: true, vulkan: false},
      evidence: "https://github.com/nerves-project/nerves_system_rpi5/tree/v2.0.1",
      note: "Stock KMS overlay and Mesa GLES; no Vulkan loader/driver configured."
    },
    %{
      version: "2.0.1",
      app: "nerves_system_rpi4",
      board: "Raspberry Pi 4 / 400",
      capabilities: %{atomic_kms: true, opengl: true, vulkan: false},
      evidence: "https://github.com/nerves-project/nerves_system_rpi4/tree/v2.0.1",
      note: "Stock KMS overlay and Mesa GLES; no Vulkan loader/driver configured."
    },
    %{
      version: "2.0.1",
      app: "nerves_system_rpi3",
      board: "Raspberry Pi 3",
      capabilities: %{atomic_kms: false, opengl: false, vulkan: false},
      evidence: "https://github.com/nerves-project/nerves_system_rpi3/tree/v2.0.1",
      note: "Stock image does not configure the KMS/Mesa GPU rendering stack."
    },
    %{
      version: "2.0.1",
      app: "nerves_system_rpi0_2",
      board: "Raspberry Pi Zero 2 W",
      capabilities: %{atomic_kms: false, opengl: false, vulkan: false},
      evidence: "https://github.com/nerves-project/nerves_system_rpi0_2/tree/v2.0.1",
      note: "Stock image does not configure the KMS/Mesa GPU rendering stack."
    },
    %{
      version: "2.0.1",
      app: "nerves_system_rpi0",
      board: "Raspberry Pi Zero / Zero W",
      capabilities: %{atomic_kms: false, opengl: false, vulkan: false},
      evidence: "https://github.com/nerves-project/nerves_system_rpi0/tree/v2.0.1",
      note: "Stock image does not configure the KMS/Mesa GPU rendering stack."
    }
  ],
  feature_order: ["drm", "wayland", "opengl", "vulkan", "macos"],
  gnu: ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
}
