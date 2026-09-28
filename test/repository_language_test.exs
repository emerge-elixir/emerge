defmodule Emerge.RepositoryLanguageTest do
  use ExUnit.Case, async: true

  test "repository-owned Python code is prohibited" do
    root = Path.expand("..", __DIR__)

    {output, 0} =
      System.cmd(
        "git",
        ["ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.py", "*.pyw"],
        cd: root
      )

    # Deleted index entries may remain until staged. External build dependencies
    # and ignored Cargo outputs are not repository-owned scripts.
    files =
      output
      |> String.split(<<0>>, trim: true)
      |> Enum.filter(&File.regular?(Path.join(root, &1)))

    assert files == [], "Use Elixir scripts or Mix tasks instead: #{inspect(files)}"
  end
end
