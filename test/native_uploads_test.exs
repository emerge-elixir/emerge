Code.require_file("../mix/native_uploads.exs", __DIR__)
Code.require_file("../mix/native_artifacts.exs", __DIR__)

defmodule Emerge.NativeUploadsTest do
  use ExUnit.Case, async: true
  import ExUnit.CaptureIO
  alias Emerge.Mix.NativeUploads

  setup do
    root = Path.join(System.tmp_dir!(), "emerge-uploads-#{System.unique_integer([:positive])}")
    directory = Path.join(root, "archives")
    File.mkdir_p!(directory)
    uploader = Path.join(root, "official-uploader.js")
    File.write!(uploader, "official uploader fixture")
    on_exit(fn -> File.rm_rf!(root) end)
    %{directory: directory, uploader: uploader}
  end

  test "every archive gets its own named upload with only its own files", context do
    names = [
      "libemerge_skia-v0.5.1-nif-2.15-aarch64-unknown-linux-gnu--opengl.so.tar.gz",
      "libemerge_skia-v0.5.1-nif-2.15-aarch64-unknown-linux-gnu--drm_opengl.so.tar.gz",
      "macos_host-v0.5.1-aarch64-apple-darwin.tar.gz"
    ]

    for name <- names, do: File.write!(Path.join(context.directory, name), name)
    checksum = Path.join(context.directory, List.last(names) <> ".sha256")
    File.write!(checksum, "host checksum")
    uploads = NativeUploads.uploads!(context.directory)
    assert Enum.map(uploads, & &1.name) == Enum.sort(names)

    capture_io(fn ->
      NativeUploads.upload!(context.directory, context.uploader, "/node24", 3,
        run: fn "/node24", [uploader], opts ->
          assert uploader == context.uploader
          env = Map.new(opts[:env])
          name = env["INPUT_NAME"]
          expected = Path.join(context.directory, name)
          files = String.split(env["INPUT_PATH"], "\n")
          assert hd(files) == expected

          if String.starts_with?(name, "macos_host-"),
            do: assert(files == [expected, checksum]),
            else: assert(files == [expected])

          assert env["INPUT_IF-NO-FILES-FOUND"] == "error"
          assert env["INPUT_COMPRESSION-LEVEL"] == "0"
          assert env["INPUT_OVERWRITE"] == "false"
          assert env["INPUT_INCLUDE-HIDDEN-FILES"] == "false"
          send(self(), {:uploaded, name})
          {"", 0}
        end
      )
    end)

    for name <- names, do: assert_received({:uploaded, ^name})
    refute_received {:uploaded, _}
  end

  test "archive counts and host checksums are validated before uploading", context do
    assert_raise RuntimeError, ~r/No native archives/, fn ->
      NativeUploads.upload!(context.directory, context.uploader, "/node24", 1)
    end

    host = Path.join(context.directory, "macos_host-v0.5.1-aarch64-apple-darwin.tar.gz")
    File.write!(host, "host archive")

    assert_raise RuntimeError, ~r/Missing host checksum/, fn ->
      NativeUploads.upload!(context.directory, context.uploader, "/node24", 1)
    end

    File.write!(host <> ".sha256", "checksum")

    assert_raise RuntimeError, ~r/Expected 2 native archives, found 1/, fn ->
      NativeUploads.upload!(context.directory, context.uploader, "/node24", 2)
    end
  end

  test "an upload failure aborts instead of reporting a complete artifact set", context do
    for name <- ~w(a.tar.gz b.tar.gz), do: File.write!(Path.join(context.directory, name), name)

    capture_io(fn ->
      assert_raise RuntimeError, ~r/Artifact upload failed for a.tar.gz \(1\)/, fn ->
        NativeUploads.upload!(context.directory, context.uploader, "/node24", 2,
          run: fn _, _, opts ->
            send(self(), {:attempted, Map.new(opts[:env])["INPUT_NAME"]})
            {"failure", 1}
          end
        )
      end
    end)

    assert_received {:attempted, "a.tar.gz"}
    refute_received {:attempted, "b.tar.gz"}
  end

  test "standalone upload bridge preserves runtime credentials for each child", context do
    names = ~w(a.tar.gz b.tar.gz)
    for name <- names, do: File.write!(Path.join(context.directory, name), name)
    log = Path.join(context.directory, "uploads.log")

    # Probe runtime credentials here. Node-only hyphenated inputs are checked
    # in the upload command unit test, not through Elixir's shell launcher.
    probe = Path.join(context.directory, "upload-probe.exs")

    File.write!(probe, """
    for {key, expected} <- [
          {"ACTIONS_RUNTIME_TOKEN", "fixture-runtime-token"},
          {"ACTIONS_RESULTS_URL", "https://example.invalid/results"}
        ] do
      unless System.get_env(key) == expected,
        do: raise("Upload environment was not preserved for " <> key)
    end

    File.write!(
      System.fetch_env!("UPLOAD_LOG"),
      System.fetch_env!("INPUT_NAME") <> "\\n",
      [:append]
    )
    """)

    {output, status} =
      System.cmd(
        "elixir",
        [
          "scripts/upload-native-artifacts.exs",
          context.directory,
          probe,
          System.find_executable("elixir"),
          "2"
        ],
        cd: Path.expand("..", __DIR__),
        stderr_to_stdout: true,
        env: [
          {"ERL_FLAGS", "+S 2:2"},
          {"ACTIONS_RUNTIME_TOKEN", "fixture-runtime-token"},
          {"ACTIONS_RESULTS_URL", "https://example.invalid/results"},
          {"UPLOAD_LOG", log}
        ]
      )

    assert status == 0, output
    assert File.read!(log) |> String.split("\n", trim: true) == names
  end

  test "all six builders expose 48 independent archive names" do
    alias Emerge.Mix.NativeArtifacts
    alias EmergeSkia.BuildProfiles

    names =
      Enum.map(BuildProfiles.release_jobs(), &NativeArtifacts.archive_name(&1, "0.5.1")) ++
        Enum.map(BuildProfiles.host_release_jobs(), &"macos_host-v0.5.1-#{&1.target}.tar.gz")

    assert length(names) == 48
    assert length(Enum.uniq(names)) == 48
    assert length(BuildProfiles.release_builders()) == 6
  end
end
