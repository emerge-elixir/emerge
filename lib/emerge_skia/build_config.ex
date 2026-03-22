defmodule EmergeSkia.BuildConfig do
  @moduledoc false

  @force_precompiled_build_env_key "EMERGE_SKIA_BUILD"
  @precompiled_targets ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"]
  @precompiled_nif_versions ["2.15"]
  @valid_backends [:wayland, :drm]

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

                               if Map.get(env, "NERVES_SDK_SYSROOT") not in [nil, ""] or
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
  def precompiled_targets, do: @precompiled_targets

  @doc false
  def precompiled_nif_versions, do: @precompiled_nif_versions

  @doc false
  def default_compiled_backends(env) when is_map(env) do
    if nerves_build_env?(env), do: [:drm], else: [:wayland]
  end

  @doc false
  def nerves_build_env?(env) when is_map(env) do
    value_present?(Map.get(env, "NERVES_SDK_SYSROOT")) ||
      nerves_compiler?(Map.get(env, "CC")) ||
      target_env?(env)
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
  def precompiled_variants(env \\ System.get_env()) when is_map(env) do
    %{
      "aarch64-unknown-linux-gnu" => [
        nerves_rpi5: fn _config -> nerves_build_env?(env) end
      ]
    }
  end

  @doc false
  def precompiled_backends(env \\ System.get_env()) when is_map(env) do
    if nerves_build_env?(env), do: [:drm], else: [:wayland]
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
      unsupported_precompiled_target?(target_resolver, targets, nif_versions) ||
      normalize_compiled_backends!(compiled_backends) != precompiled_backends(env)
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
    compiler
    |> String.split(~r/\s+/, trim: true)
    |> List.first()
    |> case do
      nil ->
        false

      path ->
        path
        |> Path.basename()
        |> String.split("-")
        |> Enum.drop(-1)
        |> Enum.join("-")
        |> case do
          "armv6-nerves-linux-gnueabihf" -> true
          "armv7-nerves-linux-gnueabihf" -> true
          "aarch64-nerves-linux-gnu" -> true
          "x86_64-nerves-linux-musl" -> true
          _other -> false
        end
    end
  end

  defp target_env?(env) do
    case {Map.get(env, "TARGET_ARCH"), Map.get(env, "TARGET_OS")} do
      {arch, os} when is_binary(arch) and arch != "" and is_binary(os) and os != "" -> true
      _ -> false
    end
  end

  defp default_precompiled_target_resolver(targets, nif_versions) do
    RustlerPrecompiled.target(%{}, targets, nif_versions)
  end

  defp force_build_requested?(env) do
    Map.get(env, @force_precompiled_build_env_key) in ["1", "true"]
  end

  defp unsupported_precompiled_target?(target_resolver, targets, nif_versions) do
    case target_resolver.(targets, nif_versions) do
      {:ok, _target} -> false
      {:error, _reason} -> true
    end
  end

  defp value_present?(value) when is_binary(value), do: value != ""
  defp value_present?(_value), do: false
end
