defmodule EmergeSkia.BuildConfig do
  @moduledoc false

  @version Mix.Project.config()[:version]
  @force_precompiled_build_env_key "EMERGE_SKIA_BUILD"
  @checksum_only_env_key "EMERGE_SKIA_CHECKSUM_ONLY"
  @github_token_env_key "EMERGE_SKIA_GITHUB_TOKEN"
  @precompiled_source_url_env_key "EMERGE_SKIA_PRECOMPILED_SOURCE_URL"
  @precompiled_targets ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
  @precompiled_nif_versions ["2.15"]
  @valid_backends [:wayland, :drm]
  @default_precompiled_source_url Mix.Project.config()[:source_url]

  @default_compiled_backends (
                               env = System.get_env()

                               cc_prefix =
                                 env
                                 |> Map.get("CC")
                                 |> case do
                                   nil ->
                                     nil

                                   compiler ->
                                     compiler
                                     |> String.split(~r/\s+/, trim: true)
                                     |> List.first()
                                     |> case do
                                       nil ->
                                         nil

                                       path ->
                                         path
                                         |> Path.basename()
                                         |> String.split("-")
                                         |> Enum.drop(-1)
                                         |> Enum.join("-")
                                     end
                                 end

                               has_target_env? =
                                 case {Map.get(env, "TARGET_ARCH"), Map.get(env, "TARGET_OS")} do
                                   {arch, os}
                                   when is_binary(arch) and arch != "" and is_binary(os) and
                                          os != "" ->
                                     true

                                   _ ->
                                     false
                                 end

                               has_mix_target? =
                                 case Map.get(env, "MIX_TARGET") do
                                   target when is_binary(target) and target not in ["", "host"] ->
                                     true

                                   _ ->
                                     false
                                 end

                               if Map.get(env, "NERVES_SDK_SYSROOT") not in [nil, ""] or
                                    has_mix_target? or
                                    cc_prefix in [
                                      "armv6-nerves-linux-gnueabihf",
                                      "armv7-nerves-linux-gnueabihf",
                                      "aarch64-nerves-linux-gnu",
                                      "x86_64-nerves-linux-musl"
                                    ] or has_target_env? do
                                 [:drm]
                               else
                                 [:wayland]
                               end
                             )

  @compiled_backends (
                       backends =
                         Application.compile_env(
                           :emerge,
                           :compiled_backends,
                           @default_compiled_backends
                         )

                       invalid_entries =
                         if is_list(backends) do
                           Enum.reject(backends, &(&1 in @valid_backends))
                         else
                           []
                         end

                       cond do
                         not is_list(backends) ->
                           raise ArgumentError,
                                 "config :emerge, compiled_backends: ... must be a list of backend atoms, got: #{inspect(backends)}"

                         invalid_entries != [] ->
                           raise ArgumentError,
                                 "config :emerge, compiled_backends: ... must be a list containing only :wayland and :drm, got invalid entries: #{inspect(invalid_entries)}"

                         true ->
                           for backend <- @valid_backends, backend in backends, do: backend
                       end
                     )

  @default_runtime_backend Enum.find(@valid_backends, &(&1 in @compiled_backends)) || :wayland

  @doc false
  def default_compiled_backends, do: @default_compiled_backends

  @doc false
  def compiled_backends, do: @compiled_backends

  @doc false
  def default_runtime_backend, do: @default_runtime_backend

  @doc false
  def force_precompiled_build_env_key, do: @force_precompiled_build_env_key

  @doc false
  def checksum_only_env_key, do: @checksum_only_env_key

  @doc false
  def precompiled_targets, do: @precompiled_targets

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
    if nerves_build_env?(env), do: [:drm], else: [:wayland]
  end

  @doc false
  def nerves_build_env?(env) when is_map(env) do
    value_present?(Map.get(env, "NERVES_SDK_SYSROOT")) ||
      mix_target?(env) ||
      nerves_compiler?(Map.get(env, "CC"))
  end

  @doc false
  def normalize_compiled_backends!(backends) when is_list(backends) do
    invalid_entries = Enum.reject(backends, &(&1 in @valid_backends))

    if invalid_entries != [] do
      raise ArgumentError,
            "config :emerge, compiled_backends: ... must be a list containing only :wayland and :drm, got invalid entries: #{inspect(invalid_entries)}"
    end

    for backend <- @valid_backends, backend in backends, do: backend
  end

  def normalize_compiled_backends!(other) do
    raise ArgumentError,
          "config :emerge, compiled_backends: ... must be a list of backend atoms, got: #{inspect(other)}"
  end

  @doc false
  def compiled_backends_to_rustler_features(backends) do
    backends
    |> normalize_compiled_backends!()
    |> Enum.map(&Atom.to_string/1)
  end

  @doc false
  def precompiled_variants(env \\ System.get_env(), compiled_backends \\ compiled_backends())
      when is_map(env) and is_list(compiled_backends) do
    %{
      "x86_64-unknown-linux-gnu" => [
        drm: fn _config ->
          precompiled_variant?(env, compiled_backends, "x86_64-unknown-linux-gnu", :drm)
        end,
        drm_wayland: fn _config ->
          precompiled_variant?(env, compiled_backends, "x86_64-unknown-linux-gnu", :drm_wayland)
        end
      ],
      "aarch64-unknown-linux-gnu" => [
        drm: fn _config ->
          precompiled_variant?(env, compiled_backends, "aarch64-unknown-linux-gnu", :drm)
        end,
        drm_wayland: fn _config ->
          precompiled_variant?(env, compiled_backends, "aarch64-unknown-linux-gnu", :drm_wayland)
        end
      ]
    }
  end

  @doc false
  def precompiled_profile(env, compiled_backends, target)
      when is_map(env) and is_list(compiled_backends) and is_binary(target) do
    compiled_backends = normalize_compiled_backends!(compiled_backends)

    cond do
      compiled_backends == [:wayland] and target in @precompiled_targets ->
        {:ok, %{target: target, variant: nil, backends: compiled_backends}}

      target == "x86_64-unknown-linux-gnu" and compiled_backends == [:drm] ->
        {:ok, %{target: target, variant: :drm, backends: compiled_backends}}

      target == "x86_64-unknown-linux-gnu" and compiled_backends == [:wayland, :drm] ->
        {:ok, %{target: target, variant: :drm_wayland, backends: compiled_backends}}

      target == "aarch64-unknown-linux-gnu" and compiled_backends == [:drm] ->
        {:ok, %{target: target, variant: :drm, backends: compiled_backends}}

      target == "aarch64-unknown-linux-gnu" and compiled_backends == [:wayland, :drm] ->
        {:ok, %{target: target, variant: :drm_wayland, backends: compiled_backends}}

      true ->
        {:error, :unsupported_profile}
    end
  end

  @doc false
  def precompiled_source_url(env \\ System.get_env()) when is_map(env) do
    Map.get(env, @precompiled_source_url_env_key, @default_precompiled_source_url)
  end

  @doc false
  def precompiled_tar_gz_url(file_name), do: precompiled_tar_gz_url(file_name, System.get_env())

  @doc false
  def precompiled_tar_gz_url(file_name, env) when is_binary(file_name) and is_map(env) do
    source_url = precompiled_source_url(env)
    direct_url = "#{source_url}/releases/download/v#{@version}/#{file_name}"

    case github_release_asset_request(source_url, @version, file_name, env) do
      {:ok, request} -> request
      :error -> maybe_authenticated_direct_url(direct_url, env)
    end
  end

  @doc false
  def force_precompiled_build?(opts \\ []) when is_list(opts) do
    env = Keyword.get(opts, :env, System.get_env())
    checksum_path = Keyword.fetch!(opts, :checksum_path)
    compiled_backends = Keyword.get(opts, :compiled_backends, compiled_backends())
    targets = Keyword.get(opts, :targets, @precompiled_targets)
    nif_versions = Keyword.get(opts, :nif_versions, @precompiled_nif_versions)
    target_resolver = Keyword.get(opts, :target_resolver, &default_precompiled_target_resolver/2)

    force_build_requested?(env) ||
      not File.exists?(checksum_path) ||
      unsupported_precompiled_profile?(
        env,
        compiled_backends,
        target_resolver,
        targets,
        nif_versions
      )
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

  defp nerves_compiler?(nil), do: false

  defp nerves_compiler?(compiler) do
    case compiler_prefix(compiler) do
      "armv6-nerves-linux-gnueabihf" -> true
      "armv7-nerves-linux-gnueabihf" -> true
      "aarch64-nerves-linux-gnu" -> true
      "x86_64-nerves-linux-musl" -> true
      _other -> false
    end
  end

  defp mix_target?(env) do
    case Map.get(env, "MIX_TARGET") do
      target when is_binary(target) and target not in ["", "host"] -> true
      _ -> false
    end
  end

  defp default_precompiled_target_resolver(targets, nif_versions) do
    RustlerPrecompiled.target(%{}, targets, nif_versions)
  end

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

  defp unsupported_precompiled_profile?(
         env,
         compiled_backends,
         target_resolver,
         targets,
         nif_versions
       ) do
    with {:ok, nif_target} <- target_resolver.(targets, nif_versions),
         {:ok, target} <- target_from_nif_target(nif_target),
         {:ok, _profile} <- precompiled_profile(env, compiled_backends, target) do
      false
    else
      _ -> true
    end
  end

  defp target_from_nif_target(nif_target) when is_binary(nif_target) do
    case String.split(nif_target, "-", parts: 3) do
      ["nif", _nif_version, target] when target != "" -> {:ok, target}
      _ -> {:error, :invalid_nif_target}
    end
  end

  defp precompiled_variant?(env, compiled_backends, target, variant) do
    case precompiled_profile(env, compiled_backends, target) do
      {:ok, %{variant: ^variant}} -> true
      _ -> false
    end
  end

  defp compiler_prefix(nil), do: nil

  defp compiler_prefix(compiler) do
    compiler
    |> String.split(~r/\s+/, trim: true)
    |> List.first()
    |> case do
      nil ->
        nil

      path ->
        path
        |> Path.basename()
        |> String.split("-")
        |> Enum.drop(-1)
        |> Enum.join("-")
    end
  end

  defp value_present?(value) when is_binary(value), do: value != ""
  defp value_present?(_value), do: false
end
