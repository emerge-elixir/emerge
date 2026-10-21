defmodule Emerge.Mix.NativeUploads do
  @moduledoc false

  def uploads!(directory) do
    uploads =
      directory
      |> Path.expand()
      |> Path.join("*.tar.gz")
      |> Path.wildcard()
      |> Enum.map(fn archive ->
        unless File.regular?(archive), do: raise("Invalid native archive: #{archive}")
        checksum = archive <> ".sha256"

        if String.starts_with?(Path.basename(archive), "macos_host-") and
             not File.regular?(checksum),
           do: raise("Missing host checksum: #{checksum}")

        %{
          name: Path.basename(archive),
          files: [archive] ++ if(File.regular?(checksum), do: [checksum], else: [])
        }
      end)

    if uploads == [], do: raise("No native archives to upload in #{directory}")
    uploads
  end

  def upload!(directory, uploader, node, expected, opts \\ []) do
    unless File.regular?(uploader), do: raise("Missing official artifact uploader: #{uploader}")
    uploads = uploads!(directory)

    unless length(uploads) == expected,
      do: raise("Expected #{expected} native archives, found #{length(uploads)}")

    run = Keyword.get(opts, :run, &System.cmd/3)

    Enum.each(uploads, fn upload ->
      IO.puts("Uploading #{upload.name}")

      case run.(node, [uploader],
             into: IO.stream(:stdio, :line),
             stderr_to_stdout: true,
             env: [
               {"INPUT_NAME", upload.name},
               {"INPUT_PATH", Enum.join(upload.files, "\n")},
               {"INPUT_IF-NO-FILES-FOUND", "error"},
               {"INPUT_COMPRESSION-LEVEL", "0"},
               {"INPUT_OVERWRITE", "false"},
               {"INPUT_INCLUDE-HIDDEN-FILES", "false"}
             ]
           ) do
        {_, 0} -> :ok
        {_, status} -> raise "Artifact upload failed for #{upload.name} (#{status})"
      end
    end)
  end
end
