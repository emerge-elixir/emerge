Code.require_file("../support.exs", __DIR__)

defmodule Emerge.Benchmarks.SharedAnimation do
  @moduledoc false
  alias Emerge.Benchmarks.Support

  def matrix(closeout \\ false) do
    %{
      "cases" =>
        if(closeout,
          do: ~w(paint pixel length moving mixed upward),
          else: ~w(paint pixel length moving mixed independent coupled upward)
        ),
      "nodes" => if(closeout, do: [20_000], else: [5000, 20_000]),
      "owners" => if(closeout, do: [64], else: [1, 64]),
      "trials" => 3,
      "closeout" => closeout
    }
  end

  def runs(matrix) do
    for trial <- 1..matrix["trials"],
        nodes <- matrix["nodes"],
        owners <- matrix["owners"],
        scenario <- Support.rotate(matrix["cases"], trial - 1),
        do: {nodes, owners, scenario, trial}
  end

  def run!(output, closeout) do
    snapshot =
      output
      |> Support.prepare!("native publication, not raster/GPU/BEAM/device qualification")
      |> Support.build!(
        ~w(build --release --manifest-path native/emerge_skia/Cargo.toml --bench shared_animation
          --features bench-diagnostics --message-format=json),
        "shared_animation"
      )

    matrix = matrix(closeout)
    Support.write_json!(snapshot.output, "matrix.json", matrix)

    Enum.each(runs(matrix), fn {nodes, owners, scenario, trial} ->
      name = "#{nodes}-#{owners}-#{scenario}-#{trial}.txt"
      log = Path.join(snapshot.output, name)
      load = File.read!("/proc/loadavg") |> String.split() |> Enum.take(3) |> Enum.join(",")

      File.write!(
        log,
        "source=#{snapshot.identity} trial=#{trial} wall_time_ns=#{System.system_time(:nanosecond)} loadavg=(#{load})\n"
      )

      Support.run_logged!(snapshot.binary, [to_string(nodes), to_string(owners), scenario], log)
      IO.puts(name)
    end)

    Support.verify!(snapshot)
    IO.puts(snapshot.identity)
  end

  def summary!(output) do
    matrix_path = Path.join(output, "matrix.json")

    matrix =
      if File.exists?(matrix_path),
        do: matrix_path |> File.read!() |> JSON.decode!(),
        else: matrix()

    rows =
      for nodes <- matrix["nodes"], owners <- matrix["owners"], scenario <- matrix["cases"] do
        texts =
          Enum.map(1..matrix["trials"], fn trial ->
            File.read!(Path.join(output, "#{nodes}-#{owners}-#{scenario}-#{trial}.txt"))
          end)

        summary_row(nodes, owners, scenario, texts)
      end

    Enum.join(
      [
        "| Nodes | Owners / loops¹ | Case | Warm p50 / p95 (ms) | Release median [min–max] (ms) | Settle median (ms) | Warm RSS MiB |",
        "|---:|---:|---|---:|---:|---:|---:|"
      ] ++
        rows ++
        [
          "",
          "¹ `upward` counts looping children under one finite Content parent. Other cases count finite owners.",
          "Warm columns are medians of per-process quantiles, not pooled distributions. RSS is not exact live heap."
        ],
      "\n"
    ) <> "\n"
  end

  def summary_row(nodes, owners, scenario, texts) do
    release = values(texts, "release_us")
    {low, high} = Enum.min_max(release)

    settle =
      if String.contains?(hd(texts), "settle_us="),
        do: values(texts, "settle_us") |> Support.median() |> Support.decimal(3),
        else: "—"

    rss =
      texts
      |> Enum.map(&capture_number!(~r/warm_p50_us=.*VmRSS:\s+(\d+)/, &1))
      |> Support.median()
      |> Kernel./(1024)
      |> Support.decimal(1)

    warm_p50 = texts |> values("warm_p50_us") |> Support.median() |> Support.decimal(3)
    warm_p95 = texts |> values("warm_p95_us") |> Support.median() |> Support.decimal(3)
    median = release |> Support.median() |> Support.decimal(3)

    "| #{nodes} | #{owners} | #{scenario} | #{warm_p50} / #{warm_p95} | #{median} [#{Support.decimal(low, 3)}–#{Support.decimal(high, 3)}] | #{settle} | #{rss} |"
  end

  defp values(texts, key),
    do: Enum.map(texts, &(capture_number!(~r/\b#{Regex.escape(key)}=(\d+)/, &1) / 1000))

  defp capture_number!(pattern, text) do
    case Regex.run(pattern, text, capture: :all_but_first) do
      [value] -> String.to_integer(value)
      _ -> raise "Missing measurement #{inspect(pattern)}"
    end
  end
end
