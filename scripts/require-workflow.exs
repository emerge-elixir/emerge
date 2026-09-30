Code.require_file("../mix/workflow_gate.exs", __DIR__)

try do
  case System.argv() do
    [sha, workflow] when workflow in ["ci.yml", "build_release_artifacts.yml"] ->
      unless Regex.match?(~r/\A[0-9a-f]{40}\z/, sha), do: raise("Expected an exact commit SHA")
      repository = System.fetch_env!("GITHUB_REPOSITORY")
      fetch = fn -> Emerge.Mix.WorkflowGate.runs!(repository, workflow, sha) end
      Emerge.Mix.WorkflowGate.await!(fetch, sha, repository, 120)

    _ ->
      raise "usage: elixir scripts/require-workflow.exs SHA {ci.yml|build_release_artifacts.yml}"
  end
rescue
  error ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
