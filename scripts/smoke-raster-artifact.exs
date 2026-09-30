Code.require_file("../mix/raster_artifact_smoke.exs", __DIR__)

case System.argv() do
  [archive] ->
    Emerge.Mix.RasterArtifactSmoke.run!(archive, Path.expand("..", __DIR__))

  _ ->
    raise "usage: mix run --no-start scripts/smoke-raster-artifact.exs ARCHIVE"
end
