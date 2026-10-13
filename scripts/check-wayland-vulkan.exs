Code.require_file("../mix/wayland_vulkan_gate.exs", __DIR__)

try do
  case System.argv() do
    ["--allow-hardware", log_path] ->
      File.cd!(Path.expand("..", __DIR__), fn ->
        Emerge.Mix.WaylandVulkanGate.run!(Path.expand(log_path))
      end)

    _ ->
      raise "usage: elixir scripts/check-wayland-vulkan.exs --allow-hardware LOG_PATH\n" <>
              "Requires a source build with Wayland Vulkan support and a running compositor."
  end
rescue
  error ->
    IO.puts(:stderr, Exception.message(error))
    System.halt(1)
end
