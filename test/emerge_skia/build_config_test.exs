defmodule EmergeSkia.BuildConfigTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildConfig

  @host_default if(:os.type() == {:unix, :darwin}, do: [:macos], else: [:wayland])

  test "normalize_compiled_backends! defaults to canonical backend order" do
    assert BuildConfig.normalize_compiled_backends!([:drm, :wayland, :drm]) == [:wayland, :drm]
  end

  test "compiled_backends_to_rustler_features returns stable feature order" do
    assert BuildConfig.compiled_backends_to_rustler_features([:drm, :wayland]) == [
             "wayland",
             "drm"
           ]
  end

  test "default_compiled_backends uses drm when NERVES_SDK_SYSROOT is present" do
    assert BuildConfig.default_compiled_backends(%{"NERVES_SDK_SYSROOT" => "/tmp/nerves/staging"}) ==
             [:drm]
  end

  test "default_compiled_backends uses drm for non-host MIX_TARGET values" do
    assert BuildConfig.default_compiled_backends(%{"MIX_TARGET" => "rpi5"}) == [:drm]
  end

  test "default_compiled_backends uses drm for known Nerves compiler prefixes" do
    assert BuildConfig.default_compiled_backends(%{"CC" => "aarch64-nerves-linux-gnu-gcc"}) ==
             [:drm]
  end

  test "default_compiled_backends does not treat generic target env as nerves" do
    assert BuildConfig.default_compiled_backends(%{
             "TARGET_ARCH" => "aarch64",
             "TARGET_OS" => "linux",
             "TARGET_ABI" => "gnu"
           }) == [:wayland]
  end

  test "default_compiled_backends uses wayland outside Nerves build environments" do
    assert BuildConfig.default_compiled_backends(%{}) == @host_default
    assert BuildConfig.default_compiled_backends(%{"MIX_TARGET" => "host"}) == @host_default
  end

  test "normalize_compiled_backends! accepts an empty backend list" do
    assert BuildConfig.normalize_compiled_backends!([]) == []
    assert BuildConfig.compiled_backends_to_rustler_features([]) == []
  end

  test "default_runtime_backend prefers wayland and falls back to drm" do
    assert BuildConfig.default_runtime_backend([:drm, :wayland]) == :wayland
    assert BuildConfig.default_runtime_backend([:drm]) == :drm
    assert BuildConfig.default_runtime_backend([]) == :wayland
  end

  test "precompiled_profile resolves x86_64 backend profiles" do
    assert {:ok, %{variant: nil, backends: [:wayland]}} =
             BuildConfig.precompiled_profile(%{}, [:wayland], "x86_64-unknown-linux-gnu")

    assert {:ok, %{variant: :drm, backends: [:drm]}} =
             BuildConfig.precompiled_profile(%{}, [:drm], "x86_64-unknown-linux-gnu")

    assert {:ok, %{variant: :drm_wayland, backends: [:wayland, :drm]}} =
             BuildConfig.precompiled_profile(%{}, [:wayland, :drm], "x86_64-unknown-linux-gnu")
  end

  test "precompiled_profile resolves aarch64 host and nerves profiles" do
    host_env = %{"TARGET_ARCH" => "aarch64", "TARGET_OS" => "linux"}

    nerves_env = %{
      "NERVES_SDK_SYSROOT" => "/tmp/nerves/staging",
      "TARGET_ARCH" => "aarch64",
      "TARGET_OS" => "linux"
    }

    assert {:ok, %{variant: nil, backends: [:wayland]}} =
             BuildConfig.precompiled_profile(host_env, [:wayland], "aarch64-unknown-linux-gnu")

    assert {:ok, %{variant: :drm, backends: [:drm]}} =
             BuildConfig.precompiled_profile(host_env, [:drm], "aarch64-unknown-linux-gnu")

    assert {:ok, %{variant: :drm_wayland, backends: [:wayland, :drm]}} =
             BuildConfig.precompiled_profile(
               host_env,
               [:wayland, :drm],
               "aarch64-unknown-linux-gnu"
             )

    assert {:ok, %{variant: :drm, backends: [:drm]}} =
             BuildConfig.precompiled_profile(nerves_env, [:drm], "aarch64-unknown-linux-gnu")
  end

  test "precompiled_variants mark exact x64 and aarch64 variants" do
    x64_variants = BuildConfig.precompiled_variants(%{}, [:wayland, :drm])
    assert x64_variants["x86_64-unknown-linux-gnu"][:drm_wayland].(%{})
    refute x64_variants["x86_64-unknown-linux-gnu"][:drm].(%{})

    host_env = %{"TARGET_ARCH" => "aarch64", "TARGET_OS" => "linux"}
    host_variants = BuildConfig.precompiled_variants(host_env, [:drm])
    assert host_variants["aarch64-unknown-linux-gnu"][:drm].(%{})

    nerves_env = %{
      "NERVES_SDK_SYSROOT" => "/tmp/nerves/staging",
      "TARGET_ARCH" => "aarch64",
      "TARGET_OS" => "linux"
    }

    nerves_variants = BuildConfig.precompiled_variants(nerves_env, [:drm])
    assert nerves_variants["aarch64-unknown-linux-gnu"][:drm].(%{})
  end

  test "precompiled_tar_gz_url adds github auth headers when token is set" do
    env = %{
      BuildConfig.precompiled_source_url_env_key() => "https://github.com/acme/emerge",
      BuildConfig.github_token_env_key() => "secret-token"
    }

    assert {url, headers} = BuildConfig.precompiled_tar_gz_url("demo.tar.gz", env)
    assert url =~ "/releases/download/v#{Mix.Project.config()[:version]}/demo.tar.gz"
    assert {"Authorization", "Bearer secret-token"} in headers
    assert {"User-Agent", "emerge-skia-precompiled"} in headers
  end

  test "precompiled_tar_gz_url falls back to plain release urls without a token" do
    env = %{BuildConfig.precompiled_source_url_env_key() => "https://github.com/acme/emerge"}

    assert BuildConfig.precompiled_tar_gz_url("demo.tar.gz", env) ==
             "https://github.com/acme/emerge/releases/download/v#{Mix.Project.config()[:version]}/demo.tar.gz"
  end

  test "checksum_only_mode? respects the checksum generation env var" do
    assert BuildConfig.checksum_only_mode?(%{BuildConfig.checksum_only_env_key() => "true"})
    refute BuildConfig.checksum_only_mode?(%{})
  end

  test "force_precompiled_build? forces builds when checksum is missing" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: "/tmp/emerge-missing-checksum",
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? resolves the current target without crashing" do
    assert is_boolean(
             BuildConfig.force_precompiled_build?(
               checksum_path: __ENV__.file,
               compiled_backends: [:wayland],
               env: %{}
             )
           )
  end

  test "force_precompiled_build? forces builds when backend profile is unsupported" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? uses precompiled artifacts when checksum, target, and backend match" do
    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? uses precompiled artifacts for x64 drm and drm_wayland profiles" do
    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:drm],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )

    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland, :drm],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? uses precompiled artifacts for generic aarch64 nerves drm" do
    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:drm],
             env: %{
               "NERVES_SDK_SYSROOT" => "/tmp/nerves/staging",
               "TARGET_ARCH" => "aarch64",
               "TARGET_OS" => "linux"
             },
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-aarch64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? respects the explicit force-build env var" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{BuildConfig.force_precompiled_build_env_key() => "true"},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )
  end

  test "force_precompiled_build? falls back to source builds for unsupported targets" do
    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions -> {:error, :unsupported_target} end
           )
  end

  test "normalize_compiled_backends! rejects invalid config shapes" do
    assert_raise ArgumentError, ~r/compiled_backends: .*must be a list of backend atoms/, fn ->
      BuildConfig.normalize_compiled_backends!(:wayland)
    end
  end

  test "normalize_compiled_backends! rejects invalid entries" do
    assert_raise ArgumentError, ~r/containing only :wayland, :drm, and :macos/, fn ->
      BuildConfig.normalize_compiled_backends!([:wayland, :bogus, "drm"])
    end
  end
end
