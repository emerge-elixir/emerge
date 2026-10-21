defmodule EmergeSkia.BuildRetryTest do
  use ExUnit.Case, async: true
  import ExUnit.CaptureIO
  alias EmergeSkia.BuildRetry

  @skia_failure """
  error: failed to run custom build command for `skia-bindings v0.99.0`
  process didn't exit successfully: build-script-build (exit status: 101)
  Synchronizing Skia dependencies
  fatal: unable to access 'https://chromium.googlesource.com/webm/libwebp.git/': The requested URL returned error: 503
  Exception: Thread failure detected
  thread 'main' panicked at skia-bindings/build_support/skia/config.rs:291:9:
  `skia/tools/git-sync-deps` failed
  """

  test "recognizes Skia sync, registry, curl and Git transient download errors" do
    for output <- [
          @skia_failure,
          "HTTP/2 502 Bad Gateway",
          "HTTP status 429",
          "failed to get successful HTTP response from `https://static.crates.io/example`, got 502",
          "http request returned unsuccessful status code: 503",
          "curl: (28) Operation timed out",
          "[6] Could not resolve hostname",
          "error: RPC failed; curl 92 HTTP/2 stream was not closed cleanly",
          "fatal: unable to access 'https://example.test': Connection reset by peer"
        ] do
      assert BuildRetry.transient?(output), output
    end
  end

  test "permanent and real build/test failures are not retried even after network messages" do
    for output <- [
          "fatal: unable to access 'https://example.test': The requested URL returned error: 404",
          "curl: (60) SSL certificate problem",
          @skia_failure <> "error[E0308]: mismatched types",
          @skia_failure <> "\e[31merror[E0308]\e[0m: mismatched types",
          @skia_failure <> "error: unused variable\n  --> src/lib.rs:12:3",
          @skia_failure <> "source.cpp:20:4: error: unknown type name",
          @skia_failure <> "checksum verification failed",
          @skia_failure <> "** (CompileError) lib/example.ex:12: undefined function",
          @skia_failure <> "Compilation failed due to warnings",
          @skia_failure <> "test result: FAILED. 1 passed; 1 failed",
          @skia_failure <> "Failed: 1 test"
        ] do
      refute BuildRetry.transient?(output), output
    end
  end

  test "retries only the failed command with bounded backoff and preserves its result" do
    assert {"success", 0} = simulate([{@skia_failure, 101}, {@skia_failure, 101}, {"success", 0}])

    assert events() |> Enum.filter(&match?({:sleep, _}, &1)) == [
             {:sleep, 10_000},
             {:sleep, 20_000}
           ]
  end

  test "exhaustion returns the final failure and does not sleep after the last attempt" do
    assert {@skia_failure, 101} = simulate(List.duplicate({@skia_failure, 101}, 3))
    messages = events()
    assert Enum.count(messages, &(&1 == :run)) == 3
    assert Enum.count(messages, &match?({:sleep, _}, &1)) == 2
  end

  test "successful, cancelled, compiler and integrity failures execute only once" do
    for result <- [
          {@skia_failure, 0},
          {@skia_failure, 130},
          {@skia_failure, 137},
          {@skia_failure, 143},
          {@skia_failure <> "error[E0308]", 101},
          {@skia_failure <> "checksum mismatch", 1}
        ] do
      assert simulate([result]) == result
      assert events() == [:run]
    end
  end

  test "streaming keeps full logs but retains only a bounded classification tail" do
    bytes = String.duplicate("x", 150_000) <> "\nHTTP/2 503\n"

    output =
      capture_io(fn ->
        tail = Enum.into([bytes], %BuildRetry.Output{sink: IO.stream(:stdio, :line)})
        assert byte_size(tail) == 131_072
        assert BuildRetry.transient?(tail)
      end)

    assert output == bytes
  end

  test "standalone CLI retries Skia-style failures without Mix or network access" do
    directory = Path.join(System.tmp_dir!(), "emerge-retry-#{System.unique_integer([:positive])}")
    File.mkdir_p!(directory)
    on_exit(fn -> File.rm_rf!(directory) end)
    script = Path.join(directory, "download.sh")

    File.write!(script, """
    if [ -e "$1" ]; then printf 'download succeeded\n'; exit 0; fi
    touch "$1"
    printf '%s\n' "fatal: unable to access 'https://example.test/': The requested URL returned error: 503"
    exit 101
    """)

    {output, status} =
      System.cmd(
        "elixir",
        ["scripts/retry-build.exs", "--delay", "0", "sh", script, Path.join(directory, "state")],
        cd: Path.expand("..", __DIR__),
        env: [{"ERL_FLAGS", "+S 2:2"}],
        stderr_to_stdout: true
      )

    assert status == 0, output
    assert output =~ "attempt 2/3"
    assert output =~ "download succeeded"
  end

  test "CI retries build preparation, not test executions" do
    root = Path.expand("..", __DIR__)
    ci = File.read!(Path.join(root, "ci-tests.sh"))
    assert ci =~ "retry_build mix compile --force --warnings-as-errors"
    assert ci =~ "retry_build cargo clippy"
    assert ci =~ "retry_build cargo test --release --no-run"
    refute ci =~ "retry_build mix test"
    refute ci =~ "retry_build cargo test --release --manifest-path"

    for file <- ~w(ci.yml build_release_artifacts.yml) do
      workflow = File.read!(Path.join([root, ".github/workflows", file]))
      assert workflow =~ ~s(CARGO_NET_RETRY: "5")
    end
  end

  defp simulate(results) do
    Enum.each(results, &send(self(), {:result, &1}))

    BuildRetry.cmd("cargo", ["clippy"], [],
      run: fn "cargo", ["clippy"], [] ->
        send(self(), :run)
        receive do: ({:result, result} -> result)
      end,
      sleep: &send(self(), {:sleep, &1}),
      report: &send(self(), {:notice, &1})
    )
  end

  defp events(acc \\ []) do
    receive do
      event -> events([event | acc])
    after
      0 -> Enum.reverse(acc)
    end
  end
end
