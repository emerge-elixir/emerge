defmodule Emerge.ReleaseWorkflowTest do
  use ExUnit.Case, async: true

  @workflows Path.expand("../.github/workflows", __DIR__)

  test "release builds depend on existing CI instead of invoking it again" do
    ci = File.read!(Path.join(@workflows, "ci.yml"))
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))
    hex = File.read!(Path.join(@workflows, "private_hex_release.yml"))

    refute ci =~ "workflow_call:"
    refute ci =~ "    tags:"
    assert ci =~ "mix test --only full_sweep --warnings-as-errors"

    for workflow <- [artifacts, hex] do
      refute workflow =~ "uses: ./.github/workflows/ci.yml"
      refute workflow =~ "mix test"
      assert workflow =~ ~s|require-workflow.exs "$(git rev-parse HEAD)" ci.yml|
      assert workflow =~ "actions: read"
    end

    for job <- ["build_release", "build_macos_host"] do
      assert artifacts =~ "  #{job}:\n    needs: validate\n"
    end

    assert hex =~ ~s|require-workflow.exs "$(git rev-parse HEAD)" build_release_artifacts.yml|
    assert artifacts =~ "needs: [validate, build_release, build_macos_host, verify_rpi5]"
    assert artifacts =~ "sha256sum --check"

    [[package]] =
      Regex.scan(
        ~r|https://github.com/nerves-project/nerves_system_rpi5/releases/download/[^/]+/([^/\s]+)\.tar\.gz|,
        artifacts,
        capture: :all_but_first
      )

    assert artifacts =~ "#{package}/images/rootfs.squashfs"
    assert artifacts =~ "for profile in drm drm_vulkan drm_all; do"
    assert artifacts =~ ~s(sudo chroot "$root" /emerge-smoke/load-nif)
  end

  test "every GNU DRM profile builds embedded Skia and checks the packaged ELF" do
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))

    for platform <- ["linux", "linux_aarch64"], profile <- ["drm", "drm_vulkan", "drm_all"] do
      [block] =
        Regex.run(
          ~r/          - name: #{platform}_#{profile}\n.*?(?=          - name:)/s,
          artifacts
        )

      assert block =~ "embedded_skia: true"
      assert block =~ "embedded-freetype"
      assert block =~ "--no-default-features"
      refute block =~ "fontconfig"
      refute block =~ "xkbcommon"
      refute block =~ "wayland"
    end

    assert artifacts =~ "SKIA_GN_ARGS=skia_use_fontconfig=false skia_use_system_freetype2=false"
    assert artifacts =~ "FORCE_SKIA_BUILD=1"
    assert artifacts =~ "elixir scripts/check-embedded-artifact.exs"
    assert artifacts =~ "uses: dtolnay/rust-toolchain@1.91.0"
  end
end
