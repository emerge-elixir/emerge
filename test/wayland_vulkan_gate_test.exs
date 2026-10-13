Code.require_file("../mix/wayland_vulkan_gate.exs", __DIR__)

defmodule Emerge.WaylandVulkanGateTest do
  use ExUnit.Case, async: true
  alias Emerge.Mix.WaylandVulkanGate, as: Gate

  test "accepts only hardware Vulkan with completed, clean sync-validation sessions" do
    assert :ok = Gate.check(clean_log(), 0)
    # Reports can complete in a different order, and native probes must be checked too.
    assert :ok = Gate.check(clean_log() <> session("4"), 0)
  end

  test "ExUnit success cannot hide native errors or warnings in any instance" do
    for replacement <- ["errors=4 warnings=0 id=2", "errors=0 warnings=1 id=2"] do
      output = String.replace(clean_log(), "errors=0 warnings=0 id=2", replacement)
      assert {:error, reason} = Gate.check(output, 0)
      assert reason =~ "native Vulkan"
    end

    assert {:error, _} = Gate.check(clean_log() <> "Vulkan validation severity=0x1000: hazard", 0)

    assert {:error, _} =
             Gate.check(clean_log() <> "Wayland Vulkan resources quarantined: timeout", 0)
  end

  test "requires both enablement and teardown, not just zero-looking summaries" do
    for output <- [
          "3 tests, 0 failures",
          String.replace(clean_log(), "sync=true", "sync=false"),
          String.replace(clean_log(), "Vulkan validation started:", "missing start:"),
          String.replace(clean_log(), "Vulkan validation final count:", "missing finish:"),
          String.replace(clean_log(), "errors=0 warnings=0 id=2", "errors=0 warnings=0 id=99"),
          clean_log() <> "Vulkan validation started: id=4 sync=true\n",
          clean_log() <> "Vulkan validation final count: errors=0 warnings=0 id=4\n"
        ] do
      assert {:error, _} = Gate.check(output, 0)
    end
  end

  test "rejects nonzero exit, duplicate reports, skipped tests and missing hardware proof" do
    assert {:error, _} = Gate.check(clean_log(), 1)
    assert {:error, _} = Gate.check(clean_log() <> session("1"), 0)

    assert {:error, _} =
             Gate.check(
               clean_log() <> "Vulkan validation final count: errors=0 warnings=0 id=1",
               0
             )

    assert {:error, _} =
             Gate.check(
               String.replace(clean_log(), "Wayland Vulkan hardware confirmed:", "GL selected:"),
               0
             )

    assert {:error, _} = Gate.check(clean_log() <> "Wayland Vulkan hardware confirmed:", 0)
  end

  test "runner cannot open a window without explicit hardware consent" do
    script = Path.expand("../scripts/check-wayland-vulkan.exs", __DIR__)
    {output, status} = System.cmd("elixir", [script], stderr_to_stdout: true)
    assert status != 0
    assert output =~ "--allow-hardware"
    refute output =~ "Wayland Vulkan selected hardware"
  end

  defp clean_log do
    "3 tests, 0 failures\n" <>
      String.duplicate("Wayland Vulkan hardware confirmed: test-device\n", 3) <>
      Enum.map_join(1..3, &"Vulkan validation started: id=#{&1} sync=true\n") <>
      Enum.map_join(3..1//-1, &"Vulkan validation final count: errors=0 warnings=0 id=#{&1}\n")
  end

  defp session(id) do
    "Vulkan validation started: id=#{id} sync=true\n" <>
      "Vulkan validation final count: errors=0 warnings=0 id=#{id}\n"
  end
end
