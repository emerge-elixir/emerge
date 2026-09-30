Code.require_file("../mix/workflow_gate.exs", __DIR__)

defmodule Emerge.WorkflowGateTest do
  use ExUnit.Case, async: true
  import ExUnit.CaptureIO
  alias Emerge.Mix.WorkflowGate

  @sha String.duplicate("a", 40)
  @repository "emerge-elixir/emerge"

  test "reuses an exact successful run even if another attempt failed" do
    assert {:ok, "https://example.test/run"} =
             WorkflowGate.result([run(%{"conclusion" => "failure"}), run()], @sha, @repository)
  end

  test "never trusts a PR, another commit or another repository" do
    for changes <- [
          %{"event" => "pull_request"},
          %{"event" => "workflow_run"},
          %{"head_sha" => String.duplicate("b", 40)},
          %{"head_repository" => %{"full_name" => "fork/emerge"}},
          %{"head_repository" => nil}
        ] do
      assert WorkflowGate.result([run(changes)], @sha, @repository) == :wait
    end
  end

  test "manual CI is eligible; cancelled, failed and skipped runs are not success" do
    assert {:ok, _} =
             WorkflowGate.result([run(%{"event" => "workflow_dispatch"})], @sha, @repository)

    for conclusion <- ["failure", "cancelled", "skipped", "timed_out", nil] do
      assert {:error, _} =
               WorkflowGate.result([run(%{"conclusion" => conclusion})], @sha, @repository)
    end
  end

  test "waits for an existing run without dispatching a new one" do
    send(self(), {:runs, [run(%{"status" => "in_progress", "conclusion" => nil})]})
    send(self(), {:runs, [run()]})
    fetch = fn -> receive do: ({:runs, runs} -> runs) end

    assert capture_io(fn ->
             WorkflowGate.await!(fetch, @sha, @repository, 2, fn -> :ok end)
           end) =~ "Using successful workflow"
  end

  test "missing runs time out and API errors fail closed" do
    capture_io(fn ->
      assert_raise RuntimeError, ~r/exact commit first/, fn ->
        WorkflowGate.await!(fn -> [] end, @sha, @repository, 1)
      end
    end)

    assert_raise RuntimeError, "API unavailable", fn ->
      WorkflowGate.await!(fn -> raise "API unavailable" end, @sha, @repository, 1)
    end
  end

  defp run(changes \\ %{}) do
    Map.merge(
      %{
        "head_sha" => @sha,
        "head_repository" => %{"full_name" => @repository},
        "event" => "push",
        "status" => "completed",
        "conclusion" => "success",
        "html_url" => "https://example.test/run"
      },
      changes
    )
  end
end
