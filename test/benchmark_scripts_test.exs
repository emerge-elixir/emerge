Code.require_file("../scripts/benchmarks/event-pressure/probe.exs", __DIR__)
Code.require_file("../scripts/benchmarks/shared-animation/probe.exs", __DIR__)

defmodule Emerge.BenchmarkScriptsTest do
  use ExUnit.Case, async: true
  alias Emerge.Benchmarks.{EventPressure, SharedAnimation, Support}

  setup do
    directory =
      Path.join(System.tmp_dir!(), "emerge-bench-scripts-#{System.unique_integer([:positive])}")

    File.mkdir_p!(directory)
    on_exit(fn -> File.rm_rf!(directory) end)
    %{directory: directory}
  end

  test "requires a direct exclusive flock parent" do
    assert Support.exclusive_parent?("/usr/bin/flock\0-x\0lock\0elixir\0")
    refute Support.exclusive_parent?("flock\0-s\0lock\0elixir\0")
    refute Support.exclusive_parent?("bash\0-c\0flock -x lock elixir\0")
    refute Support.exclusive_parent?("")
  end

  test "preserves the 30 event-pressure cases and repeat rotation" do
    cases = EventPressure.cases()
    assert length(cases) == 30
    assert length(Enum.uniq(cases)) == 30
    assert [1, "edit", 30, 30, "none", 0] == hd(cases)
    assert [20_000, "ime", 1000, 1000, "none", 0] in cases
    assert [1, "pointer", 0, 100_000, "none", 0] in cases
    assert [20_000, "edit", 0, 20_000, "none", 0] == List.last(cases)
    {first, rest} = Enum.split(cases, 7)
    assert Support.rotate(cases, 7) == rest ++ first
  end

  test "preserves full and closeout animation matrices" do
    assert length(SharedAnimation.runs(SharedAnimation.matrix())) == 96
    runs = SharedAnimation.runs(SharedAnimation.matrix(true))
    assert length(runs) == 18
    assert Enum.all?(runs, fn {nodes, owners, _, _} -> nodes == 20_000 and owners == 64 end)
    assert {20_000, 64, "paint", 1} == hd(runs)
    assert {20_000, 64, "pixel", 2} == Enum.find(runs, fn {_, _, _, trial} -> trial == 2 end)
  end

  test "selects the requested executable from Cargo JSON output" do
    jsonl =
      Enum.map_join(
        [
          %{reason: "build-script-executed"},
          %{reason: "compiler-artifact", target: %{name: "other"}, executable: "/wrong"},
          %{reason: "compiler-artifact", target: %{name: "probe"}, executable: nil},
          %{reason: "compiler-artifact", target: %{name: "probe"}, executable: "/immutable/probe"}
        ],
        "\n",
        &JSON.encode!/1
      )

    assert Support.artifact!(jsonl, "probe") == "/immutable/probe"
    assert_raise RuntimeError, fn -> Support.artifact!(jsonl, "missing") end

    assert Support.digest("abc") ==
             "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
  end

  test "streamed subprocess logs preserve headers and stderr and propagate failure", %{
    directory: root
  } do
    log = Path.join(root, "with spaces.log")
    File.write!(log, "header\n")

    Support.run_logged!("sh", ["-c", "printf '%s\\n' \"$PROBE\"; printf 'stderr\\n' >&2"], log, [
      {"PROBE", "value"}
    ])

    assert File.read!(log) == "header\nvalue\nstderr\n"

    assert_raise RuntimeError, ~r/status 3/, fn ->
      Support.run_logged!("sh", ["-c", "exit 3"], log)
    end
  end

  test "event summaries retain ranges, bursts, grouping order and settled counts", %{
    directory: root
  } do
    rows =
      for repeat <- 1..3 do
        %{
          "nodes" => 20_000,
          "kind" => "edit",
          "rate" => 0,
          "count" => 20_000,
          "stall" => "none",
          "full" => repeat,
          "event_queue_peak" => 4,
          "buffered_peak" => 5,
          "outbox_peak" => 6,
          "settled" => repeat < 3
        }
      end

    assert EventPressure.summary(rows) =~
             "| 20,000 | edit | 20000 burst | none | 1–3 | 4 | 5 | 6 | 2/3 |"

    assert_raise RuntimeError, fn -> EventPressure.summary(tl(rows)) end
    json = JSON.encode!(hd(rows))
    assert EventPressure.result!("test output\nPROBE #{json}\nfinished\n") == hd(rows)
    assert_raise RuntimeError, fn -> EventPressure.result!("no measurement") end
    Support.write_json!(root, "results.json", rows)
    EventPressure.summarize!(root)
    assert File.read!(Path.join(root, "table.md")) == EventPressure.summary(rows)
  end

  test "animation summaries report medians of per-process quantiles and optional settle", %{
    directory: root
  } do
    texts =
      for value <- [3000, 1000, 2000] do
        "warm_p50_us=#{value} warm_p95_us=#{value * 2} VmRSS: 2048 kB\nrelease_us=#{value}\nsettle_us=#{value}\n"
      end

    assert SharedAnimation.summary_row(20_000, 64, "paint", texts) ==
             "| 20000 | 64 | paint | 2.000 / 4.000 | 2.000 [1.000–3.000] | 2.000 | 2.0 |"

    without_settle = Enum.map(texts, &Regex.replace(~r/settle_us=\d+\n/, &1, ""))
    assert SharedAnimation.summary_row(20_000, 64, "paint", without_settle) =~ "| — | 2.0 |"
    assert Support.median([4, 1, 2, 3]) == 2.5

    Support.write_json!(root, "matrix.json", %{
      "nodes" => [20_000],
      "owners" => [64],
      "cases" => ["paint"],
      "trials" => 3
    })

    texts
    |> Enum.with_index(1)
    |> Enum.each(fn {text, trial} ->
      File.write!(Path.join(root, "20000-64-paint-#{trial}.txt"), text)
    end)

    assert SharedAnimation.summary!(root) =~ "not pooled distributions"

    assert SharedAnimation.summary!(root) =~
             SharedAnimation.summary_row(20_000, 64, "paint", texts)
  end
end
