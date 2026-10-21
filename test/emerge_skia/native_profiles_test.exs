defmodule EmergeSkia.NativeProfilesTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildInfo
  alias EmergeSkia.BuildProfiles, as: Profiles

  test "capability-based defaults prefer usable Vulkan, GL, CPU KMS, headless GL, then raster" do
    assert Profiles.default_matrix(%{atomic_kms: true, opengl: true, vulkan: true}) == [
             drm: [:vulkan]
           ]

    assert Profiles.default_matrix(%{atomic_kms: true, opengl: true, vulkan: false}) == [
             drm: [:opengl]
           ]

    assert Profiles.default_matrix(%{atomic_kms: true, opengl: false, vulkan: false}) == [
             drm: [:raster]
           ]

    assert Profiles.default_matrix(%{atomic_kms: false, opengl: true, vulkan: false}) == [
             headless: [:opengl]
           ]

    assert Profiles.default_matrix(%{atomic_kms: false, opengl: false, vulkan: false}) == []
    assert Profiles.default_matrix(%{}) == []
  end

  test "source feature validation uses the target, not the compilation host" do
    assert :ok = Profiles.validate_target!("aarch64-apple-darwin", ["macos"])
    assert :ok = Profiles.validate_target!("arm-unknown-linux-gnueabihf", ["opengl"])
    assert :ok = Profiles.validate_target!("aarch64-apple-darwin", [])

    assert_raise ArgumentError, ~r/not supported/, fn ->
      Profiles.validate_target!("aarch64-apple-darwin", ["opengl"])
    end

    assert_raise ArgumentError, ~r/not supported/, fn ->
      Profiles.validate_target!("x86_64-unknown-linux-gnu", ["macos"])
    end
  end

  test "system identity is exact and does not guess from a MIX_TARGET nickname or CPU" do
    env = %{
      "MIX_TARGET" => "custom",
      "CC" => "aarch64-nerves-linux-gnu-gcc",
      "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi5-portable-2.0.1"
    }

    assert %{reason: :known_system, matrix: [drm: [:opengl]], system: system} =
             Profiles.defaults(env)

    assert system.version == "2.0.1"
    refute system.capabilities.vulkan

    assert %{reason: :unknown_nerves_system, matrix: []} =
             Profiles.defaults(Map.delete(env, "NERVES_SYSTEM"))

    assert %{reason: :unknown_nerves_system, matrix: []} =
             Profiles.defaults(Map.put(env, "NERVES_SYSTEM", "/custom/nerves_system_rpi5"))

    assert %{reason: :unknown_nerves_system, matrix: []} =
             Profiles.defaults(
               Map.put(env, "NERVES_SYSTEM", "/artifacts/nerves_system_rpi5-portable-2.0.2")
             )
  end

  test "known stock Pi images have documented, versioned evidence, not SoC assumptions" do
    for system <- Profiles.systems() do
      assert system.evidence =~ "/#{system.app}/tree/v#{system.version}"
      assert system.note != ""
      assert system.capabilities.vulkan == false
    end

    assert %{matrix: []} =
             Profiles.defaults(%{
               "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi0_2-portable-2.0.1"
             })

    assert %{matrix: [drm: [:opengl]]} =
             Profiles.defaults(%{
               "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi4-portable-2.0.1"
             })
  end

  test "the catalog generates unique exact-set artifacts and release jobs" do
    profiles = for target <- Profiles.targets(), profile <- Profiles.profiles(target), do: profile
    jobs = Profiles.release_jobs()
    assert length(jobs) == length(profiles)
    assert length(Enum.uniq_by(profiles, &{&1.target, &1.features})) == length(profiles)
    assert length(Enum.uniq_by(jobs, &{&1.target, &1.variant})) == length(jobs)
    assert length(Enum.uniq_by(jobs, & &1.name)) == length(jobs)

    for {profile, job} <- Enum.zip(profiles, jobs) do
      assert job.cargo_features == Enum.join(profile.cargo_features, ",")
      assert job.variant == if(profile.variant, do: Atom.to_string(profile.variant), else: "")
      assert profile.features == Profiles.canonical_features(profile.features)

      if "vulkan" in profile.features and "drm" in profile.features,
        do: assert("drm-vulkan-deps" in profile.cargo_features)

      if "vulkan" in profile.features and "wayland" in profile.features,
        do: assert("wayland-vulkan-deps" in profile.cargo_features)

      if "opengl" not in profile.features, do: refute("opengl" in profile.cargo_features)
    end
  end

  test "macOS host profiles are separate from raster NIF profiles" do
    assert Enum.map(Profiles.host_release_jobs(), & &1.target) == Profiles.host_targets()

    for target <- Profiles.host_targets() do
      assert {:ok, %{transport: :macos_host, cargo_features: ["macos"]}} =
               Profiles.select(target, ["macos"], :macos_host)

      assert {:error, _} = Profiles.select(target, ["macos"], :nif)
    end

    report = BuildInfo.report(%{"TARGET_ARCH" => "aarch64", "TARGET_OS" => "darwin"})
    assert report =~ "Transport: macos_host"
    assert report =~ "Artifact variant: macos_host"
    refute report =~ "No exact artifact"

    assert BuildInfo.report(%{"TARGET_OS" => "darwin"}, [compiled_backends: []],
             target: "aarch64-apple-darwin"
           ) =~ "Transport: nif"
  end

  test "build info explains explicit empty overrides and image defaults" do
    env = %{
      "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi5-portable-2.0.1",
      "CC" => "aarch64-nerves-linux-gnu-gcc"
    }

    default = BuildInfo.report(env)
    assert default =~ "known_system"
    assert default =~ "compiled_backends: [drm: [:opengl]]"
    assert default =~ "Artifact variant: drm_opengl"
    override = BuildInfo.report(env, compiled_backends: [])
    assert override =~ "explicit override"
    assert override =~ "Optional features: []"
    assert override =~ "baseline (no suffix)"
  end

  test "standalone build info works before NIF compilation and without dependency code paths" do
    {text, status} =
      System.cmd(
        System.find_executable("elixir"),
        [
          "scripts/build-info.exs",
          "--backend",
          "headless:opengl",
          "--target",
          "aarch64-unknown-linux-gnu"
        ],
        cd: Path.expand("../..", __DIR__),
        stderr_to_stdout: true,
        env: [{"ERL_FLAGS", "+S 2:2"}, {"MIX_TARGET", "custom"}, {"NERVES_SYSTEM", nil}]
      )

    assert status == 0, text
    assert text =~ "Artifact variant: opengl"
    assert text =~ "unknown_nerves_system"
    assert text =~ "No device probing"
  end

  test "target inference follows the cross compiler, never the build host GPU" do
    assert Profiles.rust_target(%{"CC" => "/opt/bin/armv7-nerves-linux-gnueabihf-gcc"}) ==
             "armv7-unknown-linux-gnueabihf"

    assert Profiles.rust_target(%{
             "TARGET_ARCH" => "riscv64",
             "TARGET_OS" => "linux",
             "TARGET_ABI" => "gnu"
           }) == "riscv64gc-unknown-linux-gnu"

    assert Profiles.rust_target(%{"TARGET_ARCH" => "arm64", "TARGET_OS" => "darwin"}) ==
             "aarch64-apple-darwin"

    assert Profiles.rust_target(%{"MIX_TARGET" => "custom"}) == nil
  end

  test "Nerves runtime recipes validate without opening hardware" do
    {:docs_v1, _, _, _, %{"en" => guide}, _, _} = Code.fetch_docs(Emerge)

    examples = Regex.scan(~r/```elixir\n(.*?)\n```/s, guide, capture: :all_but_first)

    recipes = for [source] <- examples, String.contains?(source, "%Emerge.Config{"), do: source
    assert length(recipes) == 6

    for source <- recipes do
      {literal, _} =
        source |> String.replace(":my_app", ":emerge") |> Code.eval_string([], __ENV__)

      config = Emerge.Config.Normalize.config!(literal, Emerge)
      assert :ok = EmergeSkia.validate_config!(config)
    end
  end

  test "documentation tables are generated from the same catalog" do
    {:docs_v1, _, _, _, %{"en" => guide}, _, _} = Code.fetch_docs(Emerge)

    assert guide =~ BuildInfo.system_table()
    builds = File.read!(Path.expand("../../guides/reference/native-renderer-builds.md", __DIR__))
    assert builds =~ BuildInfo.profile_table()
  end
end
