defmodule EmergeSkia.BuildConfig do
  @moduledoc false

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

  defp value_present?(value) when is_binary(value), do: value != ""
  defp value_present?(_value), do: false
end
