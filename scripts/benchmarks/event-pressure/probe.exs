Code.require_file("../support.exs", __DIR__)

defmodule Emerge.Benchmarks.EventPressure do
  @moduledoc false
  alias Emerge.Benchmarks.Support

  def cases do
    paced =
      for nodes <- [1, 20_000],
          {kind, rates} <- [
            {"edit", [30, 125, 1000, 8000]},
            {"pointer", [1000, 8000]},
            {"ime", [30, 1000]}
          ],
          rate <- rates,
          do: [nodes, kind, rate, rate, "none", 0]

    tree =
      for nodes <- [1, 20_000], rate <- [30, 1000], do: [nodes, "edit", rate, rate, "tree", 1000]

    event =
      for {kind, rate} <- [{"edit", 30}, {"edit", 1000}, {"edit", 8000}, {"pointer", 8000}],
          do: [1, kind, rate, rate, "event", 1000]

    Enum.concat([
      paced,
      tree,
      [[1, "pointer", 8000, 8000, "tree", 1000], [1, "ime", 1000, 1000, "tree", 1000]],
      event,
      [
        [1, "edit", 0, 20_000, "none", 0],
        [1, "pointer", 0, 100_000, "none", 0],
        [1, "ime", 0, 20_000, "none", 0],
        [20_000, "edit", 0, 20_000, "none", 0]
      ]
    ])
  end

  def run!(output) do
    snapshot =
      output
      |> Support.prepare!(
        "instrumented native event/tree processing, not GPU/BEAM/physical device input"
      )
      |> Support.build!(
        ~w(test --locked --release --no-run --lib --manifest-path native/emerge_skia/Cargo.toml
          --features bench-diagnostics --message-format=json),
        "emerge_skia"
      )

    cases = cases()
    Support.write_json!(snapshot.output, "cases.json", cases)

    runs =
      for repeat <- 0..2,
          scenario <- Support.rotate(cases, repeat * 7),
          do: {repeat + 1, scenario}

    rows =
      Enum.map(runs, fn {repeat, scenario} ->
        label = Enum.join(scenario, "-") <> "-r#{repeat}"
        log = Path.join(snapshot.output, "#{label}.log")

        Support.run_logged!(
          "timeout",
          [
            "--signal=KILL",
            "90",
            snapshot.binary,
            "isolated_event_pressure_probe",
            "--ignored",
            "--nocapture",
            "--test-threads=1"
          ],
          log,
          [{"EMERGE_EVENT_PROBE", Enum.join(scenario, ",")}]
        )

        row = log |> File.read!() |> result!() |> Map.put("repeat", repeat)

        File.write!(Path.join(snapshot.output, "results.jsonl"), [JSON.encode!(row), "\n"], [
          :append
        ])

        IO.puts(
          "#{label} full #{row["full"]} buffer #{row["buffered_peak"]} outbox #{row["outbox_peak"]} settled #{row["settled"]}"
        )

        row
      end)

    Support.write_json!(snapshot.output, "results.json", rows)
    Support.verify!(snapshot)
    IO.puts("#{snapshot.identity} processes #{length(rows)}")
  end

  def result!(log) do
    case Regex.run(~r/PROBE (\{[^\n]+\})/, log, capture: :all_but_first) do
      [json] -> JSON.decode!(json)
      _ -> raise "Missing PROBE result"
    end
  end

  def summarize!(output) do
    rows = output |> Path.join("results.json") |> File.read!() |> JSON.decode!()
    File.write!(Path.join(output, "table.md"), summary(rows))
  end

  def summary(rows) do
    fields = ~w(nodes kind rate count stall)
    key = fn row -> Enum.map(fields, &Map.fetch!(row, &1)) end
    groups = Enum.group_by(rows, key)

    lines =
      rows
      |> Enum.map(key)
      |> Enum.uniq()
      |> Enum.map(fn [nodes, kind, rate, count, stall] = key ->
        runs = Map.fetch!(groups, key)
        unless length(runs) == 3, do: raise("Expected three event-pressure runs per case")

        peaks =
          Enum.map_join(
            ~w(full event_queue_peak buffered_peak outbox_peak),
            " | ",
            &span(runs, &1)
          )

        nodes = Regex.replace(~r/\B(?=(\d{3})+(?!\d))/, to_string(nodes), ",")
        offered = if rate == 0, do: "#{count} burst", else: to_string(rate)
        settled = Enum.count(runs, & &1["settled"])
        "| #{nodes} | #{kind} | #{offered} | #{stall} | #{peaks} | #{settled}/3 |"
      end)

    Enum.join(
      [
        "# Event pressure measurements",
        "",
        "Ranges across three separate release processes per case.",
        "",
        "| Nodes | Input | Offered/s (or burst) | Pause | Channel full | Incoming peak | Listener FIFO peak | Outbox peak | Settled within recovery window |",
        "|---:|---|---:|---|---:|---:|---:|---:|---:|" | lines
      ],
      "\n"
    ) <> "\n"
  end

  defp span(runs, field) do
    {low, high} = runs |> Enum.map(&Map.fetch!(&1, field)) |> Enum.min_max()

    if low == high,
      do: Support.decimal(low, 0),
      else: "#{Support.decimal(low, 0)}–#{Support.decimal(high, 0)}"
  end
end
