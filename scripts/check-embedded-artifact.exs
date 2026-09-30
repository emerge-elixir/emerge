Code.require_file("../mix/embedded_artifact.exs", __DIR__)

try do
  case System.argv() do
    [artifact, target, profile] -> Emerge.Mix.EmbeddedArtifact.verify!(artifact, target, profile)
    _ -> raise "usage: elixir scripts/check-embedded-artifact.exs LIBRARY TARGET PROFILE"
  end
rescue
  error ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
