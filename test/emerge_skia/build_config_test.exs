defmodule EmergeSkia.BuildConfigTest do
  use ExUnit.Case, async: true

  alias EmergeSkia.BuildConfig
  alias EmergeSkia.BuildConfig.Schema
  alias EmergeSkia.BuildProfiles, as: Profiles

  test "matrix entries resolve to a canonical union, without baseline features" do
    for {entries, features} <- [
          {[], []},
          {[headless: [:raster]], []},
          {[headless: [:opengl]], ["opengl"]},
          {[headless: [:vulkan]], ["vulkan"]},
          {[drm: [:raster]], ["drm"]},
          {[drm: [:opengl]], ["drm", "opengl"]},
          {[headless: [:opengl], drm: [:opengl]], ["drm", "opengl"]},
          {[wayland: [:raster]], ["wayland"]},
          {[wayland: [:opengl]], ["wayland", "opengl"]},
          {[drm: :all], ["drm", "opengl", "vulkan"]},
          {[wayland: [:vulkan], drm: [:opengl]], ["drm", "wayland", "opengl", "vulkan"]},
          {[macos: [:metal]], ["macos"]}
        ] do
      matrix = Schema.normalize!(entries)
      assert Profiles.features(matrix) == features

      assert BuildConfig.canonical_features(
               Schema.presenter_backends(matrix),
               Schema.api_backends(matrix, :vulkan),
               Schema.api_backends(matrix, :opengl)
             ) == features
    end
  end

  test "shorthand, all and ordering preserve canonical feature identity" do
    assert Profiles.features(Schema.normalize!([:drm])) == ["drm", "opengl"]
    assert Profiles.features(Schema.normalize!(headless: :all)) == ["opengl", "vulkan"]
    assert Profiles.features(Schema.normalize!([:macos])) == ["macos"]

    assert Profiles.features(Schema.normalize!(headless: [:raster], drm: [:opengl])) ==
             Profiles.features(Schema.normalize!(drm: [:opengl], headless: [:opengl]))
  end

  test "Cargo features contain only the selected primitives and required dependency glue" do
    assert BuildConfig.compiled_backends_to_rustler_features([], [], [:headless]) == ["opengl"]
    assert BuildConfig.compiled_backends_to_rustler_features([:drm], [], []) == ["drm"]

    assert BuildConfig.compiled_backends_to_rustler_features([:drm], [], [:drm, :headless]) == [
             "drm",
             "opengl"
           ]

    assert BuildConfig.compiled_backends_to_rustler_features([:wayland], [], []) == ["wayland"]

    assert BuildConfig.compiled_backends_to_rustler_features([:drm], [:drm], []) ==
             ["drm", "vulkan", "drm-vulkan-deps"]

    assert BuildConfig.compiled_backends_to_rustler_features([:wayland], [:wayland], []) ==
             ["wayland", "vulkan", "wayland-vulkan-deps"]

    assert BuildConfig.compiled_backends_to_rustler_features([], [:headless], []) == ["vulkan"]
  end

  test "input validation rejects duplicate backends, invalid APIs and deferred headless Metal" do
    assert_raise ArgumentError, ~r/must contain each backend once/, fn ->
      Schema.normalize!([:drm, drm: [:vulkan]])
    end

    assert_raise ArgumentError, ~r/unsupported APIs/, fn ->
      Schema.normalize!(headless: [:metal])
    end

    assert_raise ArgumentError, ~r/unsupported APIs/, fn ->
      Schema.normalize!(macos: [:vulkan])
    end

    assert_raise ArgumentError, ~r/requires at least one API/, fn ->
      Schema.normalize!(drm: [])
    end

    assert_raise ArgumentError, ~r/cannot be combined/, fn ->
      Schema.resolve!([drm: [:vulkan]], [], [:drm])
    end

    assert_raise ArgumentError, ~r/Unknown native features/, fn ->
      Profiles.canonical_features(["headless"])
    end
  end

  test "legacy API lists still resolve through the same feature union" do
    matrix = Schema.resolve!([:drm], [], [:drm])
    assert Profiles.features(matrix) == ["drm", "vulkan"]
    assert Profiles.features(Schema.resolve!([], [:headless], [])) == ["opengl"]
  end

  test "headless GL chooses a genuinely GL-only archive on every supported Linux target" do
    for target <- Profiles.targets(), String.contains?(target, "linux") do
      assert {:ok, %{variant: :opengl, features: ["opengl"]}} =
               BuildConfig.precompiled_profile(%{}, [], [], [:headless], target)

      assert {:ok, %{variant: :drm_opengl, features: ["drm", "opengl"]} = drm} =
               BuildConfig.precompiled_profile(%{}, [:drm], [], [:drm], target)

      assert {:ok, redundant} =
               BuildConfig.precompiled_profile(%{}, [:drm], [], [:drm, :headless], target)

      assert redundant.variant == drm.variant
      assert redundant.features == drm.features
    end
  end

  test "exact feature sets produce exactly one variant callback, baseline produces none" do
    for target <- Profiles.targets(), profile <- Profiles.profiles(target) do
      {backends, vulkan, opengl} = selections(profile.features)

      assert {:ok, selected} =
               BuildConfig.precompiled_profile(%{}, backends, vulkan, opengl, target)

      assert selected.features == profile.features
      variants = BuildConfig.precompiled_variants(%{}, backends, vulkan, opengl)[target]
      matches = for {variant, callback} <- variants, callback.(%{}), do: variant
      assert matches == if(profile.variant, do: [profile.variant], else: [])
    end
  end

  test "unsupported requests never select an archive with extra APIs or backends" do
    assert {:error, %{reason: :unsupported_features, features: ["wayland", "opengl"]}} =
             BuildConfig.precompiled_profile(
               %{},
               [:wayland],
               [],
               [:wayland],
               "armv7-unknown-linux-gnueabihf"
             )

    assert {:error, %{reason: :unsupported_features}} =
             BuildConfig.precompiled_profile(
               %{},
               [:drm],
               [:drm],
               [],
               "armv7-unknown-linux-gnueabihf"
             )

    assert {:error, %{reason: :unsupported_features}} =
             BuildConfig.precompiled_profile(%{}, [], [], [:headless], "aarch64-apple-darwin")

    assert {:error, %{reason: :unsupported_target}} = Profiles.select("unknown-linux", [])
  end

  test "defaults distinguish desktop, known Nerves images and unknown systems" do
    assert BuildConfig.default_compiled_backends(%{"TARGET_OS" => "linux"}) == [
             wayland: [:opengl]
           ]

    assert BuildConfig.default_compiled_backends(%{"TARGET_OS" => "darwin"}) == [macos: [:metal]]
    assert BuildConfig.default_compiled_backends(%{"MIX_TARGET" => "rpi5"}) == []
    assert BuildConfig.default_compiled_backends(%{"NERVES_SDK_SYSROOT" => "/sysroot"}) == []
    assert BuildConfig.default_compiled_backends(%{"CC" => "aarch64-nerves-linux-gnu-gcc"}) == []

    assert BuildConfig.default_compiled_backends(%{
             "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi5-portable-2.0.1"
           }) == [drm: [:opengl]]

    assert BuildConfig.default_compiled_backends(%{
             "NERVES_SYSTEM" => "/artifacts/nerves_system_rpi5-portable-99.0.0"
           }) == []

    assert BuildConfig.nerves_build_env?(%{"NERVES_SYSTEM" => "/custom/image"})
    refute BuildConfig.nerves_build_env?(%{"MIX_TARGET" => "host"})
  end

  test "platform modifiers do not mistake headless GL for a CPU-only profile" do
    env = %{"MIX_TARGET" => "custom"}
    assert BuildConfig.rustler_platform_features(env, [], [], []) == ["embedded-cpu"]

    assert BuildConfig.rustler_platform_features(env, [], [], [:headless]) == [
             "embedded-freetype"
           ]

    assert BuildConfig.rustler_platform_features(env, [:drm], [], []) == ["embedded-freetype"]
  end

  test "precompiled selection requires the exact version/variant checksum" do
    target = "aarch64-unknown-linux-gnu"

    name =
      "libemerge_skia-v#{Mix.Project.config()[:version]}-nif-2.15-#{target}--opengl.so.tar.gz"

    path = checksum_file(%{name => "sha256:" <> String.duplicate("0", 64)})

    opts = [
      env: %{},
      compiled_backends: [],
      compiled_vulkan_backends: [],
      compiled_opengl_backends: [:headless],
      checksum_path: path,
      target_resolver: fn _, _ -> {:ok, "nif-2.15-#{target}"} end
    ]

    refute BuildConfig.force_precompiled_build?(opts)
    assert {:ok, %{features: ["opengl"]}} = BuildConfig.precompiled_status(opts)
    File.write!(path, inspect(%{}))
    assert {:error, {:missing_checksum, ^name}} = BuildConfig.precompiled_status(opts)
    assert BuildConfig.force_precompiled_build?(opts)
    File.rm!(path)
    assert {:error, :missing_checksum_file} = BuildConfig.precompiled_status(opts)

    assert {:error, :explicit_source} =
             BuildConfig.precompiled_status(
               Keyword.put(opts, :env, %{"EMERGE_SKIA_BUILD" => "true"})
             )
  end

  test "source fallback distinguishes unsupported features and invalid checksum metadata" do
    opts = [
      env: %{},
      compiled_backends: [],
      compiled_vulkan_backends: [],
      compiled_opengl_backends: [:headless],
      checksum_path: checksum_file(:not_a_map),
      target_resolver: fn _, _ -> {:ok, "nif-2.15-armv7-unknown-linux-gnueabihf"} end
    ]

    assert {:error, :invalid_checksum_file} = BuildConfig.precompiled_status(opts)

    assert_raise ArgumentError, ~r/refusing source fallback/, fn ->
      BuildConfig.force_precompiled_build?(opts)
    end

    assert {:error, %{reason: :unsupported_features, available: available}} =
             BuildConfig.precompiled_status(
               Keyword.merge(opts,
                 compiled_backends: [:wayland],
                 compiled_opengl_backends: [:wayland]
               )
             )

    assert ["opengl"] in available
  end

  defp selections(features) do
    backends =
      for backend <- [:wayland, :drm, :macos], Atom.to_string(backend) in features, do: backend

    {backends, if("vulkan" in features, do: [:headless], else: []),
     if("opengl" in features, do: [:headless], else: [])}
  end

  defp checksum_file(contents) do
    path =
      Path.join(System.tmp_dir!(), "emerge-checksums-#{System.unique_integer([:positive])}.exs")

    File.write!(path, inspect(contents))
    on_exit(fn -> File.rm(path) end)
    path
  end

  test "normalize_compiled_backends! defaults to canonical backend order" do
    assert BuildConfig.normalize_compiled_backends!([:drm, :wayland, :drm]) == [:wayland, :drm]
  end

  test "load_native_runtime? skips Rustler on macOS-only runtime builds" do
    refute BuildConfig.load_native_runtime?(%{"TARGET_OS" => "darwin"}, [:macos], :prod)
  end

  test "load_native_runtime? keeps Rustler for tests on macOS-only builds" do
    assert BuildConfig.load_native_runtime?(%{"TARGET_OS" => "darwin"}, [:macos], :test)
  end

  test "load_native_runtime? respects explicit macOS NIF opt-in" do
    assert BuildConfig.load_native_runtime?(
             %{"TARGET_OS" => "darwin", BuildConfig.load_macos_nif_env_key() => "true"},
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

  test "Darwin target resolution selects the matching raster archive architecture" do
    for {arch, target} <- [
          {"aarch64", "aarch64-apple-darwin"},
          {"arm", "aarch64-apple-darwin"},
          {"x86_64", "x86_64-apple-darwin"}
        ] do
      assert {:ok, "nif-2.15-" <> ^target} =
               RustlerPrecompiled.target(
                 %{
                   os_type: {:unix, :darwin},
                   target_system: %{arch: arch, vendor: "apple", os: "darwin25.0.0"},
                   word_size: 8,
                   nif_version: "2.15"
                 },
                 BuildConfig.precompiled_targets(),
                 BuildConfig.precompiled_nif_versions()
               )
    end
  end

  test "explicit ARMv7 environment resolves the ARMv7 precompiled target" do
    env = %{"TARGET_ARCH" => "armv7", "TARGET_OS" => "linux", "TARGET_ABI" => "gnueabihf"}

    target_system =
      RustlerPrecompiled.maybe_override_with_env_vars(
        %{arch: "x86_64", vendor: "pc", os: "linux", abi: "gnu"},
        &Map.get(env, &1)
      )

    assert {:ok, "nif-2.15-armv7-unknown-linux-gnueabihf"} =
             RustlerPrecompiled.target(
               %{
                 os_type: {:unix, :linux},
                 target_system: target_system,
                 word_size: 8,
                 nif_version: "2.15"
               },
               BuildConfig.precompiled_targets(),
               BuildConfig.precompiled_nif_versions()
             )
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

    assert BuildConfig.normalize_compiled_vulkan_backends!([:headless, :wayland], [:wayland]) == [
             :wayland,
             :headless
           ]
  end

  test "normalize_compiled_opengl_backends! accepts presenter and independent headless routes" do
    assert BuildConfig.normalize_compiled_opengl_backends!([:headless, :drm], [:drm]) == [
             :drm,
             :headless
           ]
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
