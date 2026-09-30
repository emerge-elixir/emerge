Code.require_file("../mix/macos_raster_artifact.exs", __DIR__)

try do
  case System.argv() do
    [artifact, target] -> Emerge.Mix.MacosRasterArtifact.verify!(artifact, target)
    _ -> raise "usage: elixir scripts/check-macos-raster-artifact.exs LIBRARY TARGET"
  end
rescue
  error ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
