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

    assert artifacts =~ "  build_release:\n    needs: validate\n"
    refute artifacts =~ "  build_macos_host:"
    assert artifacts =~ "matrix.builder.name"

    assert hex =~ ~s|require-workflow.exs "$(git rev-parse HEAD)" build_release_artifacts.yml|
    assert artifacts =~ "needs: [validate, build_release, verify_rpi5]"
    assert artifacts =~ "pattern: native-linux_aarch64"
    assert artifacts =~ "sha256sum --check"

    [[package]] =
      Regex.scan(
        ~r|https://github.com/nerves-project/nerves_system_rpi5/releases/download/[^/]+/([^/\s]+)\.tar\.gz|,
        artifacts,
        capture: :all_but_first
      )

    assert artifacts =~ "#{package}/images/rootfs.squashfs"
    assert artifacts =~ "for profile in drm drm_opengl drm_vulkan drm_opengl_vulkan; do"
    assert artifacts =~ ~s(sudo chroot "$root" /emerge-smoke/load-nif)
  end

  test "CPU DRM has a separate exact artifact on every supported Linux target" do
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))
    jobs = EmergeSkia.BuildProfiles.release_jobs()

    for target <- EmergeSkia.BuildProfiles.targets(), String.contains?(target, "linux") do
      job = Enum.find(jobs, &(&1.target == target and &1.variant == "drm"))
      assert job.cpu_drm
      assert job.qualification_profile == "drm_raster"
      assert job.cargo_args =~ "--no-default-features --features drm,embedded-freetype"
      refute job.raster_only
      if job.container == "", do: assert(job.embedded_skia)
    end

    assert artifacts =~ "elixir scripts/build-native-artifacts.exs"
    runner = File.read!(Path.expand("../mix/native_artifacts.exs", __DIR__))
    assert runner =~ "if job.cpu_drm and"
    assert runner =~ "CPU DRM artifact enables a GPU/window-system dependency"
    build = File.read!(Path.expand("../native/emerge_skia/support/release/build.sh", __DIR__))
    assert build =~ "${EMERGE_CARGO_FEATURES:?catalog Cargo features required}"
    assert build =~ "${EMERGE_VERIFY_PROFILE:?catalog qualification profile required}"
  end

  test "embedded prebuilds keep the curated common profiles and leave other requests to source" do
    alias EmergeSkia.BuildConfig
    alias EmergeSkia.BuildProfiles

    jobs = BuildProfiles.release_jobs()

    for target <-
          ~w(armv7-unknown-linux-gnueabihf x86_64-unknown-linux-musl riscv64gc-unknown-linux-gnu) do
      assert for(job <- jobs, job.target == target, do: job.variant) ==
               ["", "opengl", "drm", "drm_opengl"]

      for {entries, expected} <- [
            {[], ""},
            {[headless: [:raster]], ""},
            {[headless: [:opengl]], "opengl"},
            {[drm: [:raster]], "drm"},
            {[drm: [:opengl]], "drm_opengl"},
            {[:drm], "drm_opengl"},
            {[drm: [:opengl], headless: [:opengl]], "drm_opengl"}
          ] do
        matrix = EmergeSkia.BuildConfig.Schema.normalize!(entries)
        features = BuildProfiles.features(matrix)
        assert {:ok, profile} = BuildProfiles.select(target, features)
        assert if(profile.variant, do: Atom.to_string(profile.variant), else: "") == expected
        assert Enum.count(jobs, &(&1.target == target and &1.variant == expected)) == 1
      end

      assert {:error, %{reason: :unsupported_features}} =
               BuildProfiles.select(target, ["wayland", "opengl"])

      assert {:error, %{reason: :unsupported_features}} =
               BuildProfiles.select(target, ["vulkan"])

      assert BuildConfig.force_precompiled_build?(
               env: %{},
               compiled_backends: [],
               compiled_vulkan_backends: [:headless],
               compiled_opengl_backends: [],
               checksum_path: "not-read-for-a-source-only-profile",
               target_resolver: fn _, _ -> {:ok, "nif-2.15-#{target}"} end
             )
    end
  end

  test "every release job has the exact versioned archive identity requested by configuration" do
    alias EmergeSkia.BuildConfig
    alias EmergeSkia.BuildConfig.Schema
    alias EmergeSkia.BuildProfiles

    checksum_path =
      Path.join(System.tmp_dir!(), "emerge-matrix-#{System.unique_integer([:positive])}.exs")

    File.write!(checksum_path, "%{}")
    on_exit(fn -> File.rm(checksum_path) end)

    for job <- BuildProfiles.release_jobs() do
      features = String.split(job.cargo_features, ",")
      apis = for api <- [:opengl, :vulkan], Atom.to_string(api) in features, do: api

      entries =
        for(
          presenter <- [:drm, :wayland],
          Atom.to_string(presenter) in features,
          do: {presenter, [:raster]}
        ) ++ if(apis == [], do: [], else: [headless: apis])

      matrix = Schema.normalize!(entries)
      suffix = if job.variant == "", do: "", else: "--#{job.variant}"

      name =
        "libemerge_skia-v#{Mix.Project.config()[:version]}-nif-2.15-#{job.target}#{suffix}.so.tar.gz"

      assert {:error, {:missing_checksum, ^name}} =
               BuildConfig.precompiled_status(
                 env: %{},
                 compiled_backends: Schema.presenter_backends(matrix),
                 compiled_vulkan_backends: Schema.api_backends(matrix, :vulkan),
                 compiled_opengl_backends: Schema.api_backends(matrix, :opengl),
                 checksum_path: checksum_path,
                 target_resolver: fn _, _ -> {:ok, "nif-2.15-#{job.target}"} end
               )
    end
  end

  test "cross mounts the native parent even when the bundled input dependency is disabled" do
    crate = Path.expand("../native/emerge_skia", __DIR__)
    cross = File.read!(Path.join(crate, "Cross.toml"))
    [_, build_env] = Regex.run(~r/\[build\.env\]\n(.*?)(?=\n\[target\.)/s, cross)

    assert build_env =~ ~s("EMERGE_NATIVE_SOURCE=..")
    assert build_env =~ ~s("EMERGE_INPUT_CACHE_DIR")
    assert build_env =~ ~s("EMERGE_INPUT_OFFLINE")
    assert build_env =~ ~s("CARGO_NET_OFFLINE")
    assert File.regular?(Path.expand("../emerge_input/Cargo.toml", crate))

    assert File.read!(Path.join(crate, "Cargo.toml")) =~
             ~s(emerge_input = { path = "../emerge_input", optional = true })
  end

  test "every cross-build image installs the bundled input archive tools" do
    crate = Path.expand("../native/emerge_skia", __DIR__)
    cross = File.read!(Path.join(crate, "Cross.toml"))

    for target <- ~w(aarch64-unknown-linux-gnu armv7-unknown-linux-gnueabihf) do
      [_, block] =
        Regex.run(~r/\[target\.#{Regex.escape(target)}\]\n(.*?)(?=\n\[|\z)/s, cross)

      assert block =~ "apt-get install"
      for tool <- ~w(curl tar gzip xz-utils bzip2), do: assert(block =~ tool)
    end

    for {image, xz} <- [{"riscv64", "xz-utils"}, {"musl", "xz"}] do
      dockerfile = File.read!(Path.join(crate, "support/release/Dockerfile.#{image}"))
      for tool <- ["curl", "tar", "gzip", xz, "bzip2"], do: assert(dockerfile =~ tool)
    end
  end

  test "Linux CI and artifact builds share only complete pin-keyed input archives" do
    ci = File.read!(Path.join(@workflows, "ci.yml"))
    assert ci =~ "cargo test --locked --manifest-path native/emerge_input/Cargo.toml"

    assert ci =~
             "cargo clippy --locked --all-targets --manifest-path native/emerge_input/Cargo.toml"

    key = "input-sources-v1-${{ hashFiles('native/emerge_input/sources.rs') }}"

    for file <- ["ci.yml", "build_release_artifacts.yml"] do
      workflow = File.read!(Path.join(@workflows, file))
      assert workflow =~ "EMERGE_INPUT_CACHE_DIR: ${{ github.workspace }}/.cache/emerge-input"
      assert workflow =~ "uses: actions/cache/restore@v5"
      assert workflow =~ "uses: actions/cache/save@v5"
      assert workflow =~ "path: ${{ env.EMERGE_INPUT_CACHE_DIR }}/*.tar.*"
      assert workflow =~ "key: #{key}"
      assert workflow =~ "key: ${{ steps.input-sources.outputs.cache-primary-key }}"
      assert workflow =~ "always() && !cancelled()"
      assert workflow =~ "steps.input-source-cache.outputs.complete == 'true'"

      assert workflow =~
               ~s|elixir scripts/check-input-source-cache.exs "$EMERGE_INPUT_CACHE_DIR" >> "$GITHUB_OUTPUT"|

      [_, cache] =
        Regex.run(
          ~r/- name: Restore verified input source archives\n(.*?)(?=\n      - name:)/s,
          workflow
        )

      refute cache =~ "matrix."
      refute cache =~ "restore-keys:"
      refute cache =~ "/target"
    end
  end

  test "container source cache is writable independently of the read-only checkout" do
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))
    runner = File.read!(Path.expand("../mix/native_artifacts.exs", __DIR__))
    assert artifacts =~ ~s|mkdir -p "$EMERGE_INPUT_CACHE_DIR"|
    assert runner =~ "EMERGE_INPUT_CACHE_DIR=/input-sources"
    assert runner =~ ~S|type=bind,source=#{input_cache},target=/input-sources|
    assert runner =~ ~S|type=bind,source=#{context.root},target=/source,readonly|
    assert runner =~ ~S|type=bind,source=#{cache},target=/build|
  end

  test "Linux Cargo cache saves and restores stay within the same matrix job" do
    ci = File.read!(Path.join(@workflows, "ci.yml"))
    [linux, _macos] = String.split(ci, "\n  ci_macos:", parts: 2)

    [_, cargo_cache] =
      Regex.run(
        ~r/      - name: Cache Cargo dependencies and target\n(.*?)(?=\n      - name:)/s,
        linux
      )

    prefix = "${{ runner.os }}-cargo-${{ matrix.rust }}-${{ matrix.name }}-"

    assert cargo_cache =~
             "key: #{prefix}${{ hashFiles('native/emerge_skia/Cargo.lock') }}"

    assert cargo_cache =~ "restore-keys: |\n            #{prefix}\n"
    refute cargo_cache =~ "${{ runner.os }}-cargo-${{ matrix.rust }}-\n"
  end

  test "every GNU DRM profile builds embedded Skia and checks the packaged ELF" do
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))

    for job <- EmergeSkia.BuildProfiles.release_jobs(),
        job.target in ~w(x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu),
        String.starts_with?(job.variant, "drm"),
        not String.contains?(job.variant, "wayland") do
      assert job.embedded_skia
      assert job.qualify_embedded
      assert job.cargo_args =~ "embedded-freetype"
      assert job.cargo_args =~ "--no-default-features"
      refute job.system_packages =~ "fontconfig"
      refute job.system_packages =~ "xkbcommon"
      refute job.system_packages =~ "wayland"
    end

    runner = File.read!(Path.expand("../mix/native_artifacts.exs", __DIR__))
    assert runner =~ "skia_use_fontconfig=false skia_use_system_freetype2=false"
    assert runner =~ "FORCE_SKIA_BUILD"
    assert runner =~ "scripts/check-embedded-artifact.exs"
    assert artifacts =~ "uses: dtolnay/rust-toolchain@1.91.0"
    assert artifacts =~ "builder: ${{ fromJSON(needs.validate.outputs.matrix) }}"
    assert artifacts =~ "elixir scripts/native-release-matrix.exs"
  end

  test "standalone release matrix is generated from the same exact feature catalog" do
    {json, 0} =
      System.cmd(System.find_executable("elixir"), ["scripts/native-release-matrix.exs"],
        cd: Path.expand("..", __DIR__),
        env: [{"ERL_FLAGS", "+S 2:2"}]
      )

    assert JSON.decode!(json) ==
             JSON.decode!(JSON.encode!(EmergeSkia.BuildProfiles.release_builders()))

    {artifacts, 0} =
      System.cmd(
        System.find_executable("elixir"),
        ["scripts/native-release-matrix.exs", "--artifacts"],
        cd: Path.expand("..", __DIR__),
        env: [{"ERL_FLAGS", "+S 2:2"}]
      )

    assert JSON.decode!(artifacts) ==
             JSON.decode!(JSON.encode!(EmergeSkia.BuildProfiles.release_jobs()))

    {hosts, 0} =
      System.cmd(
        System.find_executable("elixir"),
        ["scripts/native-release-matrix.exs", "--hosts"],
        cd: Path.expand("..", __DIR__),
        env: [{"ERL_FLAGS", "+S 2:2"}]
      )

    assert JSON.decode!(hosts) ==
             JSON.decode!(JSON.encode!(EmergeSkia.BuildProfiles.host_release_jobs()))
  end
end
