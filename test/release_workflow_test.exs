defmodule Emerge.ReleaseWorkflowTest do
  use ExUnit.Case, async: true

  @workflows Path.expand("../.github/workflows", __DIR__)

  test "release builds depend on existing CI instead of invoking it again" do
    ci = File.read!(Path.join(@workflows, "ci.yml"))
    artifacts = File.read!(Path.join(@workflows, "build_release_artifacts.yml"))
    hex = File.read!(Path.join(@workflows, "private_hex_release.yml"))

    refute ci =~ "workflow_call:"
    refute ci =~ "    tags:"
    assert ci =~ "mix test --only full_sweep --warnings-as-errors"

    for workflow <- [artifacts, hex] do
      refute workflow =~ "uses: ./.github/workflows/ci.yml"
      refute workflow =~ "mix test"
      assert workflow =~ ~s|require-workflow.exs "$(git rev-parse HEAD)" ci.yml|
      assert workflow =~ "actions: read"
    end

    for job <- ["build_release", "build_macos_host"] do
      assert artifacts =~ "  #{job}:\n    needs: validate\n"
    end

    assert hex =~ ~s|require-workflow.exs "$(git rev-parse HEAD)" build_release_artifacts.yml|
  end
end
