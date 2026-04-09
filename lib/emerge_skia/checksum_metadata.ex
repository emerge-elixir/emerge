defmodule EmergeSkia.ChecksumMetadata do
  @moduledoc false

  @doc false
  def ensure_written!(nif_module, opts) when is_atom(nif_module) and is_list(opts) do
    env = Keyword.get(opts, :env, System.get_env())
    metadata = metadata_from_options(opts)
    metadata_file = metadata_file_path(nif_module, env)

    if Map.equal?(metadata, read_map_from_file(metadata_file)) do
      :ok
    else
      metadata_file
      |> Path.dirname()
      |> File.mkdir_p!()

      File.write!(metadata_file, inspect(metadata, limit: :infinity, pretty: true))
    end

    :ok
  end

  @doc false
  def metadata_file_path(nif_module, env \\ System.get_env())
      when is_atom(nif_module) and is_map(env) do
    metadata_cache_dir(env)
    |> Path.join("metadata-#{nif_module}.exs")
  end

  @doc false
  def metadata_from_options(opts) when is_list(opts) do
    variants = Keyword.get(opts, :variants, %{})
    otp_app = Keyword.fetch!(opts, :otp_app)

    %{
      base_url: Keyword.fetch!(opts, :base_url),
      basename: Keyword.get(opts, :crate) || otp_app,
      crate: Keyword.get(opts, :crate),
      otp_app: otp_app,
      targets: Keyword.fetch!(opts, :targets),
      variants: variant_names(variants),
      nif_versions: Keyword.fetch!(opts, :nif_versions),
      version: Keyword.fetch!(opts, :version)
    }
  end

  defp metadata_cache_dir(env) when is_map(env) do
    case Map.get(env, "RUSTLER_PRECOMPILED_GLOBAL_CACHE_PATH") do
      path when is_binary(path) and path != "" ->
        path

      _ ->
        cache_opts = if Map.get(env, "MIX_XDG"), do: %{os: :linux}, else: %{}

        :filename.basedir(:user_cache, Path.join("rustler_precompiled", "metadata"), cache_opts)
        |> to_string()
    end
  end

  defp variant_names(variants) when is_map(variants) do
    Map.new(variants, fn {target, values} -> {target, Keyword.keys(values)} end)
  end

  defp read_map_from_file(file) do
    with {:ok, contents} <- File.read(file),
         {%{} = contents, _} <- Code.eval_string(contents) do
      contents
    else
      _ -> %{}
    end
  end
end
