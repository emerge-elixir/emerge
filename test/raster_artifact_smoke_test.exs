Code.require_file("../mix/raster_artifact_smoke.exs", __DIR__)

defmodule Emerge.RasterArtifactSmokeTest do
  use ExUnit.Case, async: true
  import ExUnit.CaptureIO
  alias EmergeSkia.BuildConfig

  @moduletag timeout: 120_000
  @root Path.expand("..", __DIR__)

  test "smoke harness loads an archive without Rustler and checks packed frames and corruption" do
    root =
      Path.join(System.tmp_dir!(), "emerge-raster-fixture-#{System.unique_integer([:positive])}")

    File.mkdir_p!(root)
    on_exit(fn -> File.rm_rf!(root) end)

    {:ok, nif_target} = RustlerPrecompiled.target()
    ["nif", _version, target] = String.split(nif_target, "-", parts: 3)

    {:ok, %{variant: variant}} = BuildConfig.precompiled_profile(%{}, [], [], [], target)
    suffix = if variant, do: "--#{variant}", else: ""
    name = "libemerge_skia-v#{Mix.Project.config()[:version]}-nif-2.15-#{target}#{suffix}.so"
    archive = Path.join(root, name <> ".tar.gz")
    # A local host NIF tests the harness, not release-profile or macOS compatibility.
    nif = File.read!(Application.app_dir(:emerge, "priv/native/emerge_skia.so"))
    :ok = :erl_tar.create(archive, [{String.to_charlist(name), nif}], [:compressed])

    assert capture_io(fn -> Emerge.Mix.RasterArtifactSmoke.run!(archive, @root) end) =~
             "load, cache reuse, packed rendering and checksum rejection passed"
  end
end
