defmodule Emerge.Mix.WaylandVulkanGate do
  @moduledoc false

  @started ~r/Vulkan validation started: id=(\d+) sync=(true|false)/
  @finished ~r/Vulkan validation final count: errors=(\d+) warnings=(\d+) id=(\d+)/
  @hardware ~r/Wayland Vulkan hardware confirmed:/

  @spec check(binary(), integer(), pos_integer()) :: :ok | {:error, binary()}
  def check(output, status, expected_renderers \\ 3) do
    starts = Regex.scan(@started, output, capture: :all_but_first)
    reports = Regex.scan(@finished, output, capture: :all_but_first)
    hardware_count = length(Regex.scan(@hardware, output))

    with :ok <- require_success(status),
         :ok <- require_hardware(hardware_count, expected_renderers),
         :ok <- require_sessions(starts, reports, hardware_count) do
      require_clean_reports(reports, output)
    end
  end

  @spec run!(binary()) :: :ok
  def run!(log_path) do
    # Explicit source build and API selection: neither a cached release nor GL fallback qualifies.
    {output, status} =
      System.cmd(
        "mix",
        [
          "test",
          "test/emerge_skia/wayland_frame_rate_test.exs",
          "test/emerge_skia/wayland_vulkan_sync_test.exs",
          "--include",
          "wayland_hardware",
          "--trace",
          "--no-color"
        ],
        stderr_to_stdout: true,
        env: [
          {"EMERGE_SKIA_BUILD", "1"},
          {"EMERGE_WAYLAND_TEST_API", "vulkan"},
          {"EMERGE_VULKAN_SYNC_VALIDATION", "1"},
          {"EMERGE_VULKAN_VALIDATION", "1"}
        ]
      )

    File.write!(log_path, output)
    IO.write(output)

    case check(output, status) do
      :ok -> IO.puts("Wayland Vulkan validation passed; complete log: #{log_path}")
      {:error, reason} -> raise "#{reason}; complete log: #{log_path}"
    end
  end

  defp require_success(0), do: :ok
  defp require_success(status), do: {:error, "qualification process exited with status #{status}"}

  defp require_hardware(actual, expected) when actual == expected, do: :ok

  defp require_hardware(actual, expected),
    do: {:error, "expected #{expected} hardware Vulkan confirmations, got #{actual}"}

  defp require_sessions(starts, reports, hardware_count) do
    started_ids = Enum.map(starts, &hd/1)
    finished_ids = Enum.map(reports, &List.last/1)
    started_set = MapSet.new(started_ids)
    finished_set = MapSet.new(finished_ids)

    cond do
      length(starts) < hardware_count ->
        {:error, "missing native validation startup reports"}

      Enum.any?(starts, fn [_id, sync] -> sync != "true" end) ->
        {:error, "synchronization validation was not enabled for every instance"}

      MapSet.size(started_set) != length(starts) or MapSet.size(finished_set) != length(reports) ->
        {:error, "duplicate native validation reports"}

      started_set != finished_set ->
        {:error, "missing or unmatched native validation teardown reports"}

      true ->
        :ok
    end
  end

  defp require_clean_reports(reports, output) do
    clean? =
      Enum.all?(reports, fn [errors, warnings, _id] -> errors == "0" and warnings == "0" end)

    if clean? and
         not String.contains?(output, ["Vulkan validation severity=", "resources quarantined:"]) do
      :ok
    else
      {:error, "native Vulkan validation errors, warnings or quarantined resources"}
    end
  end
end
