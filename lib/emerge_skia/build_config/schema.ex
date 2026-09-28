defmodule EmergeSkia.BuildConfig.Schema do
  @moduledoc false

  @backend_order [:wayland, :drm, :headless, :macos]
  @presenter_backends [:wayland, :drm, :macos]
  @gpu_api_order [:opengl, :vulkan]
  @backend_apis %{
    wayland: [:opengl, :vulkan],
    drm: [:opengl, :vulkan],
    headless: [:opengl, :vulkan],
    macos: [:metal]
  }

  @spec resolve!(list(), :unset | list(), :unset | list()) :: keyword(list(atom()))
  def resolve!(entries, legacy_opengl_backends, legacy_vulkan_backends) do
    matrix = normalize!(entries)

    if matrix_syntax?(entries) and
         (legacy_opengl_backends != :unset or legacy_vulkan_backends != :unset) do
      raise ArgumentError,
            "config :emerge, compiled_backends: [...] API entries cannot be combined with compiled_opengl_backends or compiled_vulkan_backends"
    end

    if matrix_syntax?(entries) do
      matrix
    else
      apply_legacy_api_lists!(matrix, legacy_opengl_backends, legacy_vulkan_backends)
    end
  end

  @spec normalize!(list()) :: keyword(list(atom()))
  def normalize!(entries) when is_list(entries) do
    normalized = Enum.map(entries, &normalize_entry!/1)
    duplicate_backends = duplicate_backends(normalized)

    if duplicate_backends != [] do
      raise ArgumentError,
            "config :emerge, compiled_backends: ... must contain each backend once, got duplicate entries: #{inspect(duplicate_backends)}"
    end

    for backend <- @backend_order,
        {^backend, apis} <- normalized,
        do: {backend, apis}
  end

  def normalize!(other) do
    raise ArgumentError,
          "config :emerge, compiled_backends: ... must be a list of backend atoms or {backend, APIs} entries, got: #{inspect(other)}"
  end

  @spec presenter_backends(keyword(list(atom()))) :: list(atom())
  def presenter_backends(matrix) when is_list(matrix) do
    for backend <- @presenter_backends, Keyword.has_key?(matrix, backend), do: backend
  end

  @spec api_backends(keyword(list(atom())), :opengl | :vulkan) :: list(atom())
  def api_backends(matrix, api) when is_list(matrix) and api in @gpu_api_order do
    for backend <- @backend_order,
        api in Keyword.get(matrix, backend, []),
        do: backend
  end

  defp normalize_entry!(backend) when is_atom(backend) do
    {backend, legacy_default_apis!(backend)}
  end

  defp normalize_entry!({backend, :all}) when is_atom(backend) do
    {backend, supported_apis!(backend)}
  end

  defp normalize_entry!({backend, apis}) when is_atom(backend) and is_list(apis) do
    supported = supported_apis!(backend)
    invalid = Enum.reject(apis, &(&1 in supported))

    cond do
      invalid != [] ->
        raise ArgumentError,
              "config :emerge, compiled_backends: ... contains unsupported APIs for #{inspect(backend)}: #{inspect(invalid)}; supported APIs are #{inspect(supported)}"

      apis == [] and backend != :wayland ->
        raise ArgumentError,
              "config :emerge, compiled_backends: ... requires at least one API for #{inspect(backend)}"

      true ->
        {backend, Enum.filter(supported, &(&1 in apis))}
    end
  end

  defp normalize_entry!(entry) do
    raise ArgumentError,
          "config :emerge, compiled_backends: ... entries must be backend atoms, {backend, :all}, or {backend, [APIs]}, got: #{inspect(entry)}"
  end

  defp legacy_default_apis!(:macos), do: [:metal]

  defp legacy_default_apis!(backend) when backend in [:wayland, :drm, :headless],
    do: [:opengl]

  defp legacy_default_apis!(backend), do: supported_apis!(backend)

  defp supported_apis!(backend) do
    case @backend_apis do
      %{^backend => apis} ->
        apis

      _ ->
        raise ArgumentError,
              "config :emerge, compiled_backends: ... contains an unsupported backend: #{inspect(backend)}; supported backends are #{inspect(@backend_order)}"
    end
  end

  defp duplicate_backends(entries) do
    entries
    |> Enum.frequencies_by(&elem(&1, 0))
    |> Enum.filter(fn {_backend, count} -> count > 1 end)
    |> Enum.map(&elem(&1, 0))
    |> Enum.filter(&(&1 in @backend_order))
  end

  defp matrix_syntax?(entries), do: Enum.any?(entries, &is_tuple/1)

  defp apply_legacy_api_lists!(matrix, legacy_opengl_backends, legacy_vulkan_backends) do
    opengl_backends =
      case legacy_opengl_backends do
        :unset ->
          api_backends(matrix, :opengl) ++
            if(legacy_vulkan_backends != :unset and :headless in legacy_vulkan_backends,
              do: [:headless],
              else: []
            )

        configured ->
          validate_legacy_api_backends!(configured, matrix, :compiled_opengl_backends)
      end

    vulkan_backends =
      case legacy_vulkan_backends do
        :unset -> []
        configured -> validate_legacy_api_backends!(configured, matrix, :compiled_vulkan_backends)
      end

    backends =
      matrix
      |> Keyword.keys()
      |> Kernel.++(opengl_backends)
      |> Kernel.++(vulkan_backends)
      |> Enum.uniq()

    for backend <- @backend_order,
        backend in backends,
        do:
          {backend,
           supported_apis!(backend)
           |> Enum.filter(fn api ->
             (api == :opengl and backend in opengl_backends) or
               (api == :vulkan and backend in vulkan_backends) or api == :metal
           end)}
  end

  defp validate_legacy_api_backends!(backends, matrix, key) when is_list(backends) do
    invalid = Enum.reject(backends, &(&1 in [:wayland, :drm, :headless]))
    presenters = presenter_backends(matrix)
    unavailable = Enum.reject(backends, &(&1 == :headless or &1 in presenters))

    cond do
      invalid != [] ->
        raise ArgumentError,
              "config :emerge, #{key}: ... must contain only :wayland, :drm, and :headless, got invalid entries: #{inspect(invalid)}"

      unavailable != [] ->
        raise ArgumentError,
              "config :emerge, #{key}: ... must be a subset of compiled_backends except for the presenter-independent :headless backend, got unavailable entries: #{inspect(unavailable)}"

      true ->
        for backend <- [:wayland, :drm, :headless], backend in backends, do: backend
    end
  end

  defp validate_legacy_api_backends!(other, _matrix, key) do
    raise ArgumentError,
          "config :emerge, #{key}: ... must be a list of backend atoms, got: #{inspect(other)}"
  end
end
