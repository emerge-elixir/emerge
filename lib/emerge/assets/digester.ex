defmodule Emerge.Assets.Digester do
  @moduledoc false

  @manifest_name "cache_manifest.json"
  @images_meta_name "cache_manifest_images.json"

  @default_extensions [".png", ".jpg", ".jpeg", ".webp", ".gif", ".bmp"]

  @spec compile([String.t()], String.t()) :: {:ok, non_neg_integer()} | {:error, term()}
  def compile(sources, output_path) when is_list(sources) and is_binary(output_path) do
    File.mkdir_p!(output_path)

    source_files =
      sources
      |> Enum.flat_map(&collect_source_files/1)
      |> Enum.uniq_by(fn {_source, logical, _absolute} -> logical end)

    latest = %{}
    digests = %{}
    images_meta = %{}

    {latest, digests, images_meta} =
      Enum.reduce(source_files, {latest, digests, images_meta}, fn {_source_root, logical,
                                                                    absolute},
                                                                   {latest_acc, digests_acc,
                                                                    images_acc} ->
        bin = File.read!(absolute)
        digest = Base.encode16(:erlang.md5(bin), case: :lower)
        digested_rel = digested_relative_path(logical, digest)

        write_asset(output_path, logical, digested_rel, bin)

        latest_acc = Map.put(latest_acc, logical, digested_rel)

        digests_acc =
          Map.put(digests_acc, digested_rel, %{
            "logical_path" => logical,
            "digest" => digest,
            "size" => byte_size(bin),
            "mtime" => now_epoch()
          })

        images_acc =
          Map.put(images_acc, logical, %{
            "digest_path" => digested_rel,
            "width" => nil,
            "height" => nil,
            "mime" => mime_from_extension(Path.extname(logical))
          })

        {latest_acc, digests_acc, images_acc}
      end)

    write_manifest(output_path, latest, digests)
    write_images_meta(output_path, images_meta)

    {:ok, map_size(latest)}
  rescue
    e -> {:error, e}
  end

  defp collect_source_files(source_root) when is_binary(source_root) do
    if File.dir?(source_root) do
      source_root
      |> Path.join("**/*")
      |> Path.wildcard()
      |> Enum.filter(&File.regular?/1)
      |> Enum.filter(&image_file?/1)
      |> Enum.map(fn absolute ->
        logical = Path.relative_to(absolute, source_root)
        {source_root, logical, absolute}
      end)
    else
      []
    end
  end

  defp image_file?(path) do
    ext = path |> Path.extname() |> String.downcase()
    ext in @default_extensions
  end

  defp digested_relative_path(logical, digest) do
    dir = Path.dirname(logical)
    ext = Path.extname(logical)
    base = logical |> Path.basename() |> Path.rootname()
    name = "#{base}-#{digest}#{ext}"

    case dir do
      "." -> name
      _ -> Path.join(dir, name)
    end
  end

  defp write_asset(output_root, logical_rel, digested_rel, bin) do
    logical_abs = Path.join(output_root, logical_rel)
    digested_abs = Path.join(output_root, digested_rel)

    File.mkdir_p!(Path.dirname(logical_abs))
    File.mkdir_p!(Path.dirname(digested_abs))

    File.write!(logical_abs, bin)
    File.write!(digested_abs, bin)
  end

  defp write_manifest(output_root, latest, digests) do
    json =
      JSON.encode_to_iodata!(%{
        "version" => 1,
        "latest" => latest,
        "digests" => digests
      })

    File.write!(Path.join(output_root, @manifest_name), json)
  end

  defp write_images_meta(output_root, images_meta) do
    json = JSON.encode_to_iodata!(%{"images" => images_meta})
    File.write!(Path.join(output_root, @images_meta_name), json)
  end

  defp mime_from_extension(ext) do
    case String.downcase(ext) do
      ".png" -> "image/png"
      ".jpg" -> "image/jpeg"
      ".jpeg" -> "image/jpeg"
      ".webp" -> "image/webp"
      ".gif" -> "image/gif"
      ".bmp" -> "image/bmp"
      _ -> "application/octet-stream"
    end
  end

  defp now_epoch do
    DateTime.utc_now() |> DateTime.to_unix()
  end
end
