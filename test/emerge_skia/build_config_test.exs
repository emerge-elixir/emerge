defmodule EmergeSkia.BuildConfigTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildConfig

  @host_default if(:os.type() == {:unix, :darwin}, do: [:macos], else: [:wayland])

  test "normalize_compiled_backends! defaults to canonical backend order" do
    assert BuildConfig.normalize_compiled_backends!([:drm, :wayland, :drm]) == [:wayland, :drm]
  end

  test "compiled backend matrix selects APIs per backend" do
    assert BuildConfig.normalize_compiled_backend_matrix!(drm: :all) == [
             drm: [:opengl, :vulkan]
           ]

    assert BuildConfig.normalize_compiled_backend_matrix!(
             headless: [:vulkan],
             drm: [:opengl]
           ) == [drm: [:opengl], headless: [:vulkan]]

    assert BuildConfig.normalize_compiled_backend_matrix!([:drm]) == [drm: [:opengl]]

    matrix = BuildConfig.normalize_compiled_backend_matrix!(drm: [:vulkan])
    assert EmergeSkia.BuildConfig.Schema.presenter_backends(matrix) == [:drm]
    assert EmergeSkia.BuildConfig.Schema.api_backends(matrix, :opengl) == []
    assert EmergeSkia.BuildConfig.Schema.api_backends(matrix, :vulkan) == [:drm]
  end

  test "compiled backend matrix maps OpenGL, Vulkan, and all selections to Cargo features" do
    for {matrix_config, expected_features} <- [
          {[wayland: [:opengl]], ["wayland"]},
          {[wayland: [:vulkan]], ["wayland-vulkan"]},
          {[wayland: :all], ["wayland-all"]},
          {[drm: [:opengl]], ["drm"]},
          {[drm: [:vulkan]], ["drm-vulkan"]},
          {[drm: :all], ["drm-all"]},
          {[headless: [:opengl]], ["headless-opengl"]},
          {[headless: [:vulkan]], ["headless-vulkan"]},
          {[headless: :all], ["headless-all"]},
          {[macos: :all], ["macos"]}
        ] do
      matrix = BuildConfig.normalize_compiled_backend_matrix!(matrix_config)
      presenters = EmergeSkia.BuildConfig.Schema.presenter_backends(matrix)
      opengl = EmergeSkia.BuildConfig.Schema.api_backends(matrix, :opengl)
      vulkan = EmergeSkia.BuildConfig.Schema.api_backends(matrix, :vulkan)

      assert BuildConfig.compiled_backends_to_rustler_features(
               presenters,
               vulkan,
               opengl
             ) == expected_features
    end
  end

  test "compiled backend matrix rejects duplicate backends and unsupported APIs" do
    assert_raise ArgumentError, ~r/must contain each backend once/, fn ->
      BuildConfig.normalize_compiled_backend_matrix!([:drm, drm: [:vulkan]])
    end

    assert_raise ArgumentError, ~r/unsupported APIs for :macos/, fn ->
      BuildConfig.normalize_compiled_backend_matrix!(macos: [:vulkan])
    end

    assert_raise ArgumentError, ~r/requires at least one API for :drm/, fn ->
      BuildConfig.normalize_compiled_backend_matrix!(drm: [])
    end
  end

  test "compiled backend matrix cannot be mixed with legacy API lists" do
    assert_raise ArgumentError, ~r/cannot be combined/, fn ->
      EmergeSkia.BuildConfig.Schema.resolve!([drm: [:vulkan]], [], [:drm])
    end
  end

  test "compiled_backends_to_rustler_features returns stable feature order" do
    assert BuildConfig.compiled_backends_to_rustler_features([:drm, :wayland]) == [
             "wayland",
             "drm"
           ]
  end

  test "compiled_backends_to_rustler_features enables Vulkan per backend" do
    assert BuildConfig.compiled_backends_to_rustler_features([:drm], [:drm]) == ["drm-all"]

    assert BuildConfig.compiled_backends_to_rustler_features(
             [:drm, :wayland],
             [:wayland]
           ) == ["wayland-all", "drm"]

    assert BuildConfig.compiled_backends_to_rustler_features(
             [:drm, :wayland],
             [:drm, :wayland]
           ) == ["wayland-all", "drm-all"]
  end

  test "compiled_backends_to_rustler_features enables the headless Vulkan producer independently" do
    assert BuildConfig.compiled_backends_to_rustler_features(
             [:wayland],
             [:headless]
           ) == ["wayland", "headless-all"]

    assert BuildConfig.compiled_backends_to_rustler_features(
             [:wayland],
             [:wayland, :headless]
           ) == ["wayland-all", "headless-all"]
  end

  test "compiled_backends_to_rustler_features supports Vulkan without OpenGL" do
    assert BuildConfig.compiled_backends_to_rustler_features(
             [:wayland],
             [:wayland],
             []
           ) == ["wayland-vulkan"]

    assert BuildConfig.compiled_backends_to_rustler_features([:drm], [:drm], []) == [
             "drm-vulkan"
           ]

    assert BuildConfig.compiled_backends_to_rustler_features([], [:headless], []) == [
             "headless-vulkan"
           ]
  end

  test "compiled_backends_to_rustler_features supports CPU-presented Wayland raster" do
    assert BuildConfig.compiled_backends_to_rustler_features([:wayland], [], []) == [
             "wayland-core"
           ]
  end

  test "load_native_runtime? skips Rustler on macOS-only runtime builds" do
    refute BuildConfig.load_native_runtime?(%{"TARGET_OS" => "darwin"}, [:macos], :prod)
  end

  test "load_native_runtime? keeps Rustler for tests on macOS-only builds" do
    assert BuildConfig.load_native_runtime?(%{"TARGET_OS" => "darwin"}, [:macos], :test)
  end

  test "load_native_runtime? respects explicit macOS NIF opt-in" do
    assert BuildConfig.load_native_runtime?(
             %{
               "TARGET_OS" => "darwin",
               BuildConfig.load_macos_nif_env_key() => "true"
             },
             [:macos],
             :prod
           )
  end

  test "macos_host_target resolves darwin runtime target from env" do
    assert BuildConfig.macos_host_target(%{"TARGET_ARCH" => "arm64", "TARGET_OS" => "darwin"}) ==
             "aarch64-apple-darwin"

    assert BuildConfig.macos_host_target(%{"TARGET_ARCH" => "x86_64", "TARGET_OS" => "darwin"}) ==
             "x86_64-apple-darwin"
  end

  test "macos_host artifact helpers use stable names" do
    assert BuildConfig.macos_host_archive_name("aarch64-apple-darwin", "0.2.1") ==
             "macos_host-v0.2.1-aarch64-apple-darwin.tar.gz"

    assert BuildConfig.macos_host_checksum_name("x86_64-apple-darwin", "0.2.1") ==
             "macos_host-v0.2.1-x86_64-apple-darwin.tar.gz.sha256"
  end

  test "macos_host download helpers reuse release base url" do
    env = %{BuildConfig.precompiled_source_url_env_key() => "https://github.com/acme/emerge"}

    assert BuildConfig.macos_host_download_url("aarch64-apple-darwin", env, "0.2.1") ==
             "https://github.com/acme/emerge/releases/download/v0.2.1/macos_host-v0.2.1-aarch64-apple-darwin.tar.gz"

    assert BuildConfig.macos_host_checksum_url("x86_64-apple-darwin", env, "0.2.1") ==
             "https://github.com/acme/emerge/releases/download/v0.2.1/macos_host-v0.2.1-x86_64-apple-darwin.tar.gz.sha256"
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

  test "Nerves raster-only builds select the minimal embedded CPU Rust profile" do
    nerves_env = %{"MIX_TARGET" => "trellis"}

    assert BuildConfig.rustler_platform_features(nerves_env, [], []) == ["embedded-cpu"]

    assert BuildConfig.rustler_platform_features(nerves_env, [:drm], []) == [
             "embedded-freetype"
           ]

    assert BuildConfig.rustler_platform_features(%{}, [], []) == ["video-interop-support"]
  end

  test "default_runtime_backend prefers wayland and falls back to drm" do
    assert BuildConfig.default_runtime_backend([:drm, :wayland]) == :wayland
    assert BuildConfig.default_runtime_backend([:drm]) == :drm
    assert BuildConfig.default_runtime_backend([]) == :wayland
  end

  test "precompiled targets include 64-bit Linux and 32-bit ARM hard-float" do
    assert BuildConfig.precompiled_targets() == [
             "x86_64-unknown-linux-gnu",
             "aarch64-unknown-linux-gnu",
             "arm-unknown-linux-gnueabihf"
           ]
  end

  test "Nerves ARM environment resolves the 32-bit precompiled target" do
    assert {:ok, "nif-2.15-arm-unknown-linux-gnueabihf"} =
             RustlerPrecompiled.target(
               %{
                 os_type: {:unix, :linux},
                 target_system: %{
                   arch: "arm",
                   vendor: "unknown",
                   os: "linux",
                   abi: "gnueabihf"
                 },
                 word_size: 4,
                 nif_version: "2.15"
               },
               BuildConfig.precompiled_targets(),
               BuildConfig.precompiled_nif_versions()
             )
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

  test "precompiled_profile resolves raster, Vulkan, and 32-bit ARM profiles" do
    for target <- ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] do
      assert {:ok, %{variant: :raster, backends: [], vulkan_backends: []}} =
               BuildConfig.precompiled_profile(%{}, [], [], target)

      assert {:ok, %{variant: :vulkan, vulkan_backends: [:headless]}} =
               BuildConfig.precompiled_profile(%{}, [], [:headless], target)

      assert {:ok,
              %{variant: :headless_vulkan, vulkan_backends: [:headless], opengl_backends: []}} =
               BuildConfig.precompiled_profile(%{}, [], [:headless], [], target)

      assert {:ok, %{variant: :wayland_vulkan, vulkan_backends: [:wayland], opengl_backends: []}} =
               BuildConfig.precompiled_profile(%{}, [:wayland], [:wayland], [], target)

      assert {:ok, %{variant: :drm_vulkan, vulkan_backends: [:drm], opengl_backends: []}} =
               BuildConfig.precompiled_profile(%{}, [:drm], [:drm], [], target)
    end

    assert {:ok, %{variant: nil, backends: [], vulkan_backends: []}} =
             BuildConfig.precompiled_profile(
               %{"MIX_TARGET" => "trellis"},
               [],
               [],
               "arm-unknown-linux-gnueabihf"
             )

    assert {:ok, %{variant: :opengl, backends: [:drm], vulkan_backends: []}} =
             BuildConfig.precompiled_profile(
               %{"MIX_TARGET" => "trellis"},
               [:drm],
               [],
               "arm-unknown-linux-gnueabihf"
             )

    assert {:error, :unsupported_profile} =
             BuildConfig.precompiled_profile(
               %{"MIX_TARGET" => "trellis"},
               [:drm],
               [:drm],
               "arm-unknown-linux-gnueabihf"
             )
  end

  test "precompiled_variants select exact backend and rendering profiles" do
    x64_variants = BuildConfig.precompiled_variants(%{}, [:wayland, :drm], [])
    assert x64_variants["x86_64-unknown-linux-gnu"][:drm_wayland].(%{})
    refute x64_variants["x86_64-unknown-linux-gnu"][:drm].(%{})

    raster_variants = BuildConfig.precompiled_variants(%{}, [], [])
    assert raster_variants["x86_64-unknown-linux-gnu"][:raster].(%{})
    assert raster_variants["aarch64-unknown-linux-gnu"][:raster].(%{})
    refute raster_variants["x86_64-unknown-linux-gnu"][:vulkan].(%{})

    vulkan_variants = BuildConfig.precompiled_variants(%{}, [], [:headless])
    assert vulkan_variants["x86_64-unknown-linux-gnu"][:vulkan].(%{})
    assert vulkan_variants["aarch64-unknown-linux-gnu"][:vulkan].(%{})

    vulkan_only_variants = BuildConfig.precompiled_variants(%{}, [:drm], [:drm], [])
    assert vulkan_only_variants["x86_64-unknown-linux-gnu"][:drm_vulkan].(%{})
    assert vulkan_only_variants["aarch64-unknown-linux-gnu"][:drm_vulkan].(%{})
    refute vulkan_only_variants["x86_64-unknown-linux-gnu"][:vulkan].(%{})

    arm_raster_variants =
      BuildConfig.precompiled_variants(%{"MIX_TARGET" => "trellis"}, [], [])

    refute arm_raster_variants["arm-unknown-linux-gnueabihf"][:opengl].(%{})

    arm_opengl_variants =
      BuildConfig.precompiled_variants(%{"MIX_TARGET" => "trellis"}, [:drm], [])

    assert arm_opengl_variants["arm-unknown-linux-gnueabihf"][:opengl].(%{})
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
             compiled_backends: [:macos],
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

  test "force_precompiled_build? uses 64-bit Vulkan artifacts and rejects 32-bit ARM Vulkan" do
    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:wayland],
             compiled_vulkan_backends: [:wayland],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-x86_64-unknown-linux-gnu"}
             end
           )

    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:drm],
             compiled_vulkan_backends: [:drm],
             compiled_opengl_backends: [],
             env: %{},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-aarch64-unknown-linux-gnu"}
             end
           )

    assert BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:drm],
             compiled_vulkan_backends: [:drm],
             compiled_opengl_backends: [],
             env: %{"MIX_TARGET" => "trellis"},
             target_resolver: fn _targets, _nif_versions ->
               {:ok, "nif-2.15-arm-unknown-linux-gnueabihf"}
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

  test "force_precompiled_build? uses 32-bit ARM raster and OpenGL artifacts" do
    target_resolver = fn _targets, _nif_versions ->
      {:ok, "nif-2.15-arm-unknown-linux-gnueabihf"}
    end

    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [],
             env: %{"MIX_TARGET" => "trellis"},
             target_resolver: target_resolver
           )

    refute BuildConfig.force_precompiled_build?(
             checksum_path: __ENV__.file,
             compiled_backends: [:drm],
             env: %{"MIX_TARGET" => "trellis"},
             target_resolver: target_resolver
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

  test "normalize_compiled_vulkan_backends! accepts the independent headless producer" do
    assert BuildConfig.normalize_compiled_vulkan_backends!([:headless], [:wayland]) == [:headless]

    assert BuildConfig.normalize_compiled_vulkan_backends!(
             [:headless, :wayland],
             [:wayland]
           ) == [:wayland, :headless]
  end

  test "normalize_compiled_opengl_backends! accepts presenter and independent headless routes" do
    assert BuildConfig.normalize_compiled_opengl_backends!(
             [:headless, :drm],
             [:drm]
           ) == [:drm, :headless]
  end

  test "normalize_compiled_opengl_backends! rejects invalid and unavailable backends" do
    assert_raise ArgumentError, ~r/must be a list of backend atoms/, fn ->
      BuildConfig.normalize_compiled_opengl_backends!(:drm, [:drm])
    end

    assert_raise ArgumentError, ~r/must contain only :wayland, :drm, and :headless/, fn ->
      BuildConfig.normalize_compiled_opengl_backends!([:macos], [:macos])
    end

    assert_raise ArgumentError, ~r/must be a subset of compiled_backends/, fn ->
      BuildConfig.normalize_compiled_opengl_backends!([:drm], [:wayland])
    end
  end

  test "normalize_compiled_vulkan_backends! rejects invalid shapes and unavailable backends" do
    assert_raise ArgumentError, ~r/must be a list of backend atoms/, fn ->
      BuildConfig.normalize_compiled_vulkan_backends!(:wayland, [:wayland])
    end

    assert_raise ArgumentError, ~r/must contain only :wayland, :drm, and :headless/, fn ->
      BuildConfig.normalize_compiled_vulkan_backends!([:macos], [:macos])
    end

    assert_raise ArgumentError, ~r/must be a subset of compiled_backends/, fn ->
      BuildConfig.normalize_compiled_vulkan_backends!([:drm], [:wayland])
    end
  end
end
