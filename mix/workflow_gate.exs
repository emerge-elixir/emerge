defmodule Emerge.Mix.WorkflowGate do
  @moduledoc false

  # A PR (including a fork), another SHA or another repository cannot authorize
  # publication. A previously successful run is sufficient; do not rerun tests.
  def result(runs, sha, repository) do
    eligible =
      Enum.filter(runs, fn run ->
        run["head_sha"] == sha and
          get_in(run, ["head_repository", "full_name"]) == repository and
          run["event"] in ["push", "workflow_dispatch"]
      end)

    cond do
      run = Enum.find(eligible, &(&1["status"] == "completed" and &1["conclusion"] == "success")) ->
        {:ok, run["html_url"]}

      eligible == [] or Enum.any?(eligible, &(&1["status"] != "completed")) ->
        :wait

      true ->
        {:error, "No successful workflow run for #{sha}; rerun the failed workflow explicitly"}
    end
  end

  def await!(fetch, sha, repository, attempts, sleep \\ fn -> Process.sleep(30_000) end)
      when attempts > 0 do
    case result(fetch.(), sha, repository) do
      {:ok, url} ->
        IO.puts("Using successful workflow: #{url}")

      :wait when attempts > 1 ->
        IO.puts("Waiting for an existing workflow run for #{sha} (#{attempts} checks left)")
        sleep.()
        await!(fetch, sha, repository, attempts - 1, sleep)

      :wait ->
        raise "No completed successful workflow for #{sha}; run it on this exact commit first"

      {:error, message} ->
        raise message
    end
  end

  def runs!(repository, workflow, sha) do
    endpoint =
      "repos/#{repository}/actions/workflows/#{workflow}/runs?head_sha=#{sha}&per_page=100"

    case System.cmd("gh", ["api", "--paginate", "--slurp", endpoint], stderr_to_stdout: true) do
      {json, 0} ->
        json |> :json.decode() |> Enum.flat_map(&Map.fetch!(&1, "workflow_runs"))

      {_output, status} ->
        raise "Unable to query workflow #{workflow} (gh exit #{status}); refusing release"
    end
  end
end
