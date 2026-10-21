defmodule EmergeSkia.BuildConfig do
  @moduledoc false

  @version Mix.Project.config()[:version]
  @force_precompiled_build_env_key "EMERGE_SKIA_BUILD"
  @load_macos_nif_env_key "EMERGE_SKIA_LOAD_MACOS_NIF"
  @build_local_macos_host_env_key "EMERGE_SKIA_MACOS_HOST_BUILD_LOCAL"
  @macos_host_cache_dir_env_key "EMERGE_SKIA_MACOS_HOST_CACHE_DIR"
  @checksum_only_env_key "EMERGE_SKIA_CHECKSUM_ONLY"
  @github_token_env_key "EMERGE_SKIA_GITHUB_TOKEN"
  @precompiled_source_url_env_key "EMERGE_SKIA_PRECOMPILED_SOURCE_URL"
  @macos_host_targets EmergeSkia.BuildProfiles.host_targets()
  @precompiled_targets EmergeSkia.BuildProfiles.targets()
  @precompiled_nif_versions ["2.15"]
  @valid_backends [:wayland, :drm, :macos]
  @valid_opengl_backends [:wayland, :drm, :headless]
  @valid_vulkan_backends [:wayland, :drm, :headless]
  @default_precompiled_source_url Mix.Project.config()[:source_url]

  @profiles_path Path.expand("../../mix/native_profiles.exs", __DIR__)
  @external_resource @profiles_path
  @external_resource Path.expand("../../mix/targets.exs", __DIR__)
  @default_compiled_backends EmergeSkia.BuildProfiles.defaults(System.get_env()).matrix

  @raw_compiled_backends Application.compile_env(
                           :emerge,
                           :compiled_backends,
                           :unset
                         )
  @legacy_compiled_opengl_backends Application.compile_env(
                                     :emerge,
                                     :compiled_opengl_backends,
                                     :unset
                                   )
  @legacy_compiled_vulkan_backends Application.compile_env(
                                     :emerge,
                                     :compiled_vulkan_backends,
                                     :unset
                                   )
  @compiled_backend_entries if(@raw_compiled_backends == :unset,
                              do:
                                if(
                                  @legacy_compiled_opengl_backends != :unset or
                                    @legacy_compiled_vulkan_backends != :unset,
                                  do: Keyword.keys(@default_compiled_backends),
                                  else: @default_compiled_backends
                                ),
                              else: @raw_compiled_backends
                            )
  @compiled_backend_matrix EmergeSkia.BuildConfig.Schema.resolve!(
                             @compiled_backend_entries,
                             @legacy_compiled_opengl_backends,
                             @legacy_compiled_vulkan_backends
                           )
  @compiled_backends EmergeSkia.BuildConfig.Schema.presenter_backends(@compiled_backend_matrix)
  @compiled_opengl_backends EmergeSkia.BuildConfig.Schema.api_backends(
                              @compiled_backend_matrix,
                              :opengl
                            )
  @compiled_vulkan_backends EmergeSkia.BuildConfig.Schema.api_backends(
                              @compiled_backend_matrix,
                              :vulkan
                            )

  @default_runtime_backend Enum.find(@valid_backends, &(&1 in @compiled_backends)) || :wayland

  @doc false
  def default_compiled_backends, do: @default_compiled_backends

  @doc false
  def compiled_backend_matrix, do: @compiled_backend_matrix

  @doc false
  def compiled_backends, do: @compiled_backends

  @doc false
  def compiled_vulkan_backends, do: @compiled_vulkan_backends

  @doc false
  def compiled_opengl_backends, do: @compiled_opengl_backends

  @doc false
  def default_runtime_backend, do: @default_runtime_backend

  @doc false
  def force_precompiled_build_env_key, do: @force_precompiled_build_env_key

  @doc false
  def load_macos_nif_env_key, do: @load_macos_nif_env_key

  @doc false
  def build_local_macos_host_env_key, do: @build_local_macos_host_env_key

  @doc false
  def macos_host_cache_dir_env_key, do: @macos_host_cache_dir_env_key

  @doc false
  def checksum_only_env_key, do: @checksum_only_env_key

  @doc false
  def precompiled_targets, do: @precompiled_targets

  @doc false
  def macos_host_targets, do: @macos_host_targets

  @doc false
  def precompiled_nif_versions, do: @precompiled_nif_versions

  @doc false
  def github_token_env_key, do: @github_token_env_key

  @doc false
  def precompiled_source_url_env_key, do: @precompiled_source_url_env_key

  @doc false
  def checksum_only_mode?(env \\ System.get_env()) when is_map(env) do
    Map.get(env, @checksum_only_env_key) in ["1", "true"]
  end

  @doc false
  def default_compiled_backends(env) when is_map(env) do
    EmergeSkia.BuildProfiles.defaults(env).matrix
  end

  @doc false
  def nerves_build_env?(env) when is_map(env), do: EmergeSkia.BuildProfiles.nerves?(env)

  @doc false
  def normalize_compiled_backend_matrix!(backends) do
    EmergeSkia.BuildConfig.Schema.normalize!(backends)
  end

  @doc false
  def normalize_compiled_backends!(backends) when is_list(backends) do
    invalid_entries = Enum.reject(backends, &(&1 in @valid_backends))

    if invalid_entries != [] do
      raise ArgumentError,
            "config :emerge, compiled_backends: ... must be a list containing only :wayland, :drm, and :macos, got invalid entries: #{inspect(invalid_entries)}"
    end

    for backend <- @valid_backends, backend in backends, do: backend
  end

  def normalize_compiled_backends!(other) do
    raise ArgumentError,
          "config :emerge, compiled_backends: ... must be a list of backend atoms, got: #{inspect(other)}"
  end

  @doc false
  def normalize_compiled_vulkan_backends!(vulkan_backends, compiled_backends)
      when is_list(vulkan_backends) and is_list(compiled_backends) do
    compiled_backends = normalize_compiled_backends!(compiled_backends)
    invalid_entries = Enum.reject(vulkan_backends, &(&1 in @valid_vulkan_backends))

    unavailable_entries =
      Enum.reject(vulkan_backends, &(&1 == :headless or &1 in compiled_backends))

    cond do
      invalid_entries != [] ->
        raise ArgumentError,
              "config :emerge, compiled_vulkan_backends: ... must contain only :wayland, :drm, and :headless, got invalid entries: #{inspect(invalid_entries)}"

      unavailable_entries != [] ->
        raise ArgumentError,
              "config :emerge, compiled_vulkan_backends: ... must be a subset of compiled_backends except for the presenter-independent :headless backend, got unavailable entries: #{inspect(unavailable_entries)}"

      true ->
        for backend <- @valid_vulkan_backends, backend in vulkan_backends, do: backend
    end
  end

  def normalize_compiled_vulkan_backends!(other, _compiled_backends) do
    raise ArgumentError,
          "config :emerge, compiled_vulkan_backends: ... must be a list of backend atoms, got: #{inspect(other)}"
  end

  @doc false
  def normalize_compiled_opengl_backends!(opengl_backends, compiled_backends)
      when is_list(opengl_backends) and is_list(compiled_backends) do
    compiled_backends = normalize_compiled_backends!(compiled_backends)
    invalid_entries = Enum.reject(opengl_backends, &(&1 in @valid_opengl_backends))

    unavailable_entries =
      Enum.reject(opengl_backends, &(&1 == :headless or &1 in compiled_backends))

    cond do
      invalid_entries != [] ->
        raise ArgumentError,
              "config :emerge, compiled_opengl_backends: ... must contain only :wayland, :drm, and :headless, got invalid entries: #{inspect(invalid_entries)}"

      unavailable_entries != [] ->
        raise ArgumentError,
              "config :emerge, compiled_opengl_backends: ... must be a subset of compiled_backends except for the presenter-independent :headless backend, got unavailable entries: #{inspect(unavailable_entries)}"

      true ->
        for backend <- @valid_opengl_backends, backend in opengl_backends, do: backend
    end
  end

  def normalize_compiled_opengl_backends!(other, _compiled_backends) do
    raise ArgumentError,
          "config :emerge, compiled_opengl_backends: ... must be a list of backend atoms, got: #{inspect(other)}"
  end

  @doc false
  def rustler_platform_features(env, backends, vulkan, opengl \\ :default) do
    EmergeSkia.BuildProfiles.platform_features(env, canonical_features(backends, vulkan, opengl))
  end

  @doc false
  def canonical_features(backends, vulkan \\ [], opengl \\ :default) do
    backends = normalize_compiled_backends!(backends)
    vulkan = normalize_compiled_vulkan_backends!(vulkan, backends)

    opengl =
      case opengl do
        :default -> default_opengl_backends(backends, vulkan)
        configured -> normalize_compiled_opengl_backends!(configured, backends)
      end

    (Enum.map(backends, &Atom.to_string/1) ++
       if(opengl != [], do: ["opengl"], else: []) ++
       if(vulkan != [], do: ["vulkan"], else: []))
    |> EmergeSkia.BuildProfiles.canonical_features()
  end

  @doc false
  def compiled_backends_to_rustler_features(backends, vulkan \\ [], opengl \\ :default) do
    backends |> canonical_features(vulkan, opengl) |> EmergeSkia.BuildProfiles.cargo_features()
  end

  @doc false
  def load_native_runtime?(
        env \\ System.get_env(),
        compiled_backends \\ compiled_backends(),
        mix_env \\ Mix.env()
      )
      when is_map(env) and is_list(compiled_backends) do
    compiled_backends = normalize_compiled_backends!(compiled_backends)

    cond do
      Map.get(env, @load_macos_nif_env_key) in ["1", "true"] ->
        true

      mix_env == :test ->
        true

      host_darwin?(env) and compiled_backends == [:macos] ->
        false

      true ->
        true
    end
  end

  @doc false
  def precompiled_variants do
    precompiled_variants(
      System.get_env(),
      compiled_backends(),
      compiled_vulkan_backends(),
      compiled_opengl_backends()
    )
  end

  @doc false
  def precompiled_variants(env) when is_map(env) do
    precompiled_variants(
      env,
      compiled_backends(),
      compiled_vulkan_backends(),
      compiled_opengl_backends()
    )
  end

  @doc false
  def precompiled_variants(env, compiled_backends) do
    precompiled_variants(env, compiled_backends, [], :default)
  end

  @doc false
  def precompiled_variants(env, compiled_backends, compiled_vulkan_backends) do
    precompiled_variants(env, compiled_backends, compiled_vulkan_backends, :default)
  end

  @doc false
  def precompiled_variants(
        env,
        compiled_backends,
        compiled_vulkan_backends,
        compiled_opengl_backends
      )
      when is_map(env) and is_list(compiled_backends) and
             is_list(compiled_vulkan_backends) do
    features =
      canonical_features(compiled_backends, compiled_vulkan_backends, compiled_opengl_backends)

    Map.new(@precompiled_targets, fn target ->
      variants =
        for profile <- EmergeSkia.BuildProfiles.profiles(target), profile.variant != nil do
          {profile.variant, fn _config -> profile.features == features end}
        end

      {target, variants}
    end)
  end

  @doc false
  def precompiled_profile(env, compiled_backends, target) do
    precompiled_profile(env, compiled_backends, [], :default, target)
  end

  @doc false
  def precompiled_profile(env, compiled_backends, compiled_vulkan_backends, target) do
    precompiled_profile(env, compiled_backends, compiled_vulkan_backends, :default, target)
  end

  @doc false
  def precompiled_profile(
        env,
        compiled_backends,
        compiled_vulkan_backends,
        compiled_opengl_backends,
        target
      )
      when is_map(env) and is_list(compiled_backends) and
             is_list(compiled_vulkan_backends) and is_binary(target) do
    features =
      canonical_features(compiled_backends, compiled_vulkan_backends, compiled_opengl_backends)

    case EmergeSkia.BuildProfiles.select(target, features) do
      {:ok, profile} ->
        {:ok,
         Map.merge(profile, %{
           backends: compiled_backends,
           vulkan_backends: compiled_vulkan_backends,
           opengl_backends:
             case compiled_opengl_backends do
               :default -> default_opengl_backends(compiled_backends, compiled_vulkan_backends)
               configured -> configured
             end
         })}

      {:error, reason} ->
        {:error, reason}
    end
  end

  @doc false
  def precompiled_source_url(env \\ System.get_env()) when is_map(env) do
    Map.get(env, @precompiled_source_url_env_key, @default_precompiled_source_url)
  end

  @doc false
  def macos_host_target(env \\ System.get_env()) when is_map(env) do
    case {Map.get(env, "TARGET_ARCH"), Map.get(env, "TARGET_OS")} do
      {arch, os} when is_binary(arch) and arch != "" and os == "darwin" ->
        macos_target_from_arch(arch)

      _ ->
        current_target_system()
        |> Map.get(:arch)
        |> macos_target_from_arch()
    end
  end

  @doc false
  def macos_host_archive_name(target, version \\ @version)
      when is_binary(target) and is_binary(version) do
    "macos_host-v#{version}-#{target}.tar.gz"
  end

  @doc false
  def macos_host_checksum_name(target, version \\ @version)
      when is_binary(target) and is_binary(version) do
    "#{macos_host_archive_name(target, version)}.sha256"
  end

  @doc false
  def macos_host_download_url(target, env \\ System.get_env(), version \\ @version)
      when is_binary(target) and is_map(env) and is_binary(version) do
    release_asset_url(macos_host_archive_name(target, version), version, env)
  end

  @doc false
  def macos_host_checksum_url(target, env \\ System.get_env(), version \\ @version)
      when is_binary(target) and is_map(env) and is_binary(version) do
    release_asset_url(macos_host_checksum_name(target, version), version, env)
  end

  @doc false
  def macos_host_cache_dir(target, version \\ @version, env \\ System.get_env())
      when is_binary(target) and is_binary(version) and is_map(env) do
    base =
      case Map.get(env, @macos_host_cache_dir_env_key) do
        path when is_binary(path) and path != "" -> path
        _ -> user_cache_dir()
      end

    Path.join([base, "emerge_skia", "macos_host", version, target])
  end

  @doc false
  def precompiled_tar_gz_url(file_name), do: precompiled_tar_gz_url(file_name, System.get_env())

  @doc false
  def precompiled_tar_gz_url(file_name, env) when is_binary(file_name) and is_map(env) do
    release_asset_url(file_name, @version, env)
  end

  @doc false
  def force_precompiled_build?(opts \\ []) do
    case precompiled_status(opts) do
      {:ok, _profile} ->
        false

      {:error, :invalid_checksum_file} ->
        raise ArgumentError, "Invalid Emerge checksum manifest; refusing source fallback"

      {:error, {:checksum_file, reason}} ->
        raise File.Error,
          reason: reason,
          action: "read checksum manifest",
          path: Keyword.fetch!(opts, :checksum_path)

      {:error, reason} ->
        if Keyword.get(opts, :warn_on_fallback, false) and reason != :explicit_source do
          IO.warn(
            "Emerge native source build: #{inspect(reason)}. " <>
              "A potentially long Skia/Rust build will occur; Rustler is required. " <>
              "Run mix emerge.build_info or elixir deps/emerge/scripts/build-info.exs for feature/target details. " <>
              "See https://hexdocs.pm/emerge/Emerge.html#module-viewport-configuration. " <>
              "To request source compilation explicitly: EMERGE_SKIA_BUILD=true mix deps.compile emerge --force"
          )
        end

        true
    end
  end

  @doc false
  def precompiled_status(opts) do
    env = Keyword.get(opts, :env, System.get_env())
    backends = Keyword.get(opts, :compiled_backends, compiled_backends())
    vulkan = Keyword.get(opts, :compiled_vulkan_backends, compiled_vulkan_backends())
    opengl = Keyword.get(opts, :compiled_opengl_backends, :default)
    features = canonical_features(backends, vulkan, opengl)
    resolver = Keyword.get(opts, :target_resolver, &default_precompiled_target_resolver/2)
    targets = Keyword.get(opts, :targets, @precompiled_targets)
    versions = Keyword.get(opts, :nif_versions, @precompiled_nif_versions)

    if force_build_requested?(env) do
      {:error, :explicit_source}
    else
      with {:ok, nif_target} <- resolver.(targets, versions),
           {:ok, target} <- target_from_nif_target(nif_target),
           {:ok, profile} <- EmergeSkia.BuildProfiles.select(target, features),
           {:ok, checksums} <- read_checksums(Keyword.fetch!(opts, :checksum_path)) do
        suffix = if profile.variant, do: "--#{profile.variant}", else: ""
        name = "libemerge_skia-v#{@version}-#{nif_target}#{suffix}.so.tar.gz"

        if Map.has_key?(checksums, name),
          do: {:ok, profile},
          else: {:error, {:missing_checksum, name}}
      end
    end
  end

  defp read_checksums(path) do
    case File.read(path) do
      {:ok, text} ->
        case Code.eval_string(text) do
          {%{} = checksums, _binding} -> {:ok, checksums}
          _ -> {:error, :invalid_checksum_file}
        end

      {:error, :enoent} ->
        {:error, :missing_checksum_file}

      {:error, reason} ->
        {:error, {:checksum_file, reason}}
    end
  rescue
    _ -> {:error, :invalid_checksum_file}
  end

  @doc false
  def default_runtime_backend(backends) do
    backends
    |> normalize_compiled_backends!()
    |> case do
      [] -> :wayland
      normalized -> Enum.find(@valid_backends, &(&1 in normalized)) || :wayland
    end
  end

  defp default_opengl_backends(compiled_backends, compiled_vulkan_backends) do
    Enum.filter(compiled_backends, &(&1 in @valid_opengl_backends)) ++
      if(:headless in compiled_vulkan_backends, do: [:headless], else: [])
  end

  defp host_darwin?(env) when is_map(env) do
    case Map.get(env, "TARGET_OS") do
      os when is_binary(os) and os != "" -> os == "darwin"
      _ -> :os.type() == {:unix, :darwin}
    end
  end

  defp default_precompiled_target_resolver(targets, nif_versions) do
    RustlerPrecompiled.target(
      %{
        os_type: :os.type(),
        target_system: current_target_system(),
        word_size: :erlang.system_info(:wordsize),
        nif_version: current_compatible_nif_version(nif_versions)
      },
      targets,
      nif_versions
    )
  end

  defp current_compatible_nif_version(nif_versions) do
    current_nif_version = :erlang.system_info(:nif_version) |> List.to_string()

    case RustlerPrecompiled.find_compatible_nif_version(current_nif_version, nif_versions) do
      {:ok, version} -> version
      :error -> current_nif_version
    end
  end

  defp current_target_system do
    :erlang.system_info(:system_architecture)
    |> List.to_string()
    |> String.split("-")
    |> system_arch_parts_to_map()
    |> RustlerPrecompiled.maybe_override_with_env_vars()
  end

  defp system_arch_parts_to_map([arch, vendor, os, abi]),
    do: %{arch: arch, vendor: vendor, os: os, abi: abi}

  defp system_arch_parts_to_map([arch, vendor, os]),
    do: %{arch: arch, vendor: vendor, os: os}

  defp system_arch_parts_to_map(_parts), do: %{}

  defp force_build_requested?(env) do
    Map.get(env, @force_precompiled_build_env_key) in ["1", "true"]
  end

  defp maybe_authenticated_direct_url(url, env) do
    case Map.get(env, @github_token_env_key) do
      token when is_binary(token) and token != "" ->
        {url,
         [
           {"Authorization", "Bearer #{token}"},
           {"Accept", "application/octet-stream"},
           {"User-Agent", "emerge-skia-precompiled"}
         ]}

      _ ->
        url
    end
  end

  defp release_asset_url(file_name, version, env)
       when is_binary(file_name) and is_binary(version) and is_map(env) do
    source_url = precompiled_source_url(env)
    direct_url = "#{source_url}/releases/download/v#{version}/#{file_name}"

    case github_release_asset_request(source_url, version, file_name, env) do
      {:ok, request} -> request
      :error -> maybe_authenticated_direct_url(direct_url, env)
    end
  end

  defp github_release_asset_request(source_url, version, file_name, env) do
    with token when is_binary(token) and token != "" <- Map.get(env, @github_token_env_key),
         {:ok, owner, repo} <- github_repo(source_url),
         {:ok, asset_url} <- github_release_asset_url(owner, repo, version, file_name, token) do
      {:ok,
       {asset_url,
        [
          {"Authorization", "Bearer #{token}"},
          {"Accept", "application/octet-stream"},
          {"X-GitHub-Api-Version", "2022-11-28"},
          {"User-Agent", "emerge-skia-precompiled"}
        ]}}
    else
      _ -> :error
    end
  end

  defp github_repo(source_url) when is_binary(source_url) do
    case Regex.run(~r/^https:\/\/github\.com\/([^\/]+)\/([^\/]+?)(?:\.git)?\/?$/, source_url) do
      [_, owner, repo] -> {:ok, owner, repo}
      _ -> :error
    end
  end

  defp github_release_asset_url(owner, repo, version, file_name, token) do
    release_url = "https://api.github.com/repos/#{owner}/#{repo}/releases/tags/v#{version}"

    headers = [
      {~c"Authorization", ~c"Bearer " ++ String.to_charlist(token)},
      {~c"Accept", ~c"application/vnd.github+json"},
      {~c"X-GitHub-Api-Version", ~c"2022-11-28"},
      {~c"User-Agent", ~c"emerge-skia-precompiled"}
    ]

    :inets.start()
    :ssl.start()

    case :httpc.request(:get, {String.to_charlist(release_url), headers}, [],
           body_format: :binary
         ) do
      {:ok, {{_, 200, _}, _response_headers, body}} ->
        with {:ok, %{"assets" => assets}} <- Jason.decode(body),
             %{"url" => asset_url} <- Enum.find(assets, &(&1["name"] == file_name)) do
          {:ok, asset_url}
        else
          _ -> :error
        end

      _ ->
        :error
    end
  end

  defp target_from_nif_target(nif_target) when is_binary(nif_target) do
    case String.split(nif_target, "-", parts: 3) do
      ["nif", _nif_version, target] when target != "" -> {:ok, target}
      _ -> {:error, :invalid_nif_target}
    end
  end

  defp macos_target_from_arch("aarch64"), do: "aarch64-apple-darwin"
  defp macos_target_from_arch("arm64"), do: "aarch64-apple-darwin"
  defp macos_target_from_arch("x86_64"), do: "x86_64-apple-darwin"
  defp macos_target_from_arch("amd64"), do: "x86_64-apple-darwin"

  defp macos_target_from_arch(other) do
    raise ArgumentError, "unsupported macOS target architecture: #{inspect(other)}"
  end

  defp user_cache_dir do
    :filename.basedir(:user_cache, "", %{os: :darwin})
    |> to_string()
  end
end
