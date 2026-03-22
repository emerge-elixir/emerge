defmodule EmergeSkia.BuildConfig do
  @moduledoc false

  @valid_backends [:wayland, :drm]

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
end
