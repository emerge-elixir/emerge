defmodule EmergeDemo do
  @moduledoc """
  Example Emerge application that renders a low-latency Membrane WiFi video
  pipeline into an `EmergeSkia` video target.
  """

  @spec runtime_config() :: keyword()
  def runtime_config do
    Application.get_env(:emerge_demo, EmergeDemo.Runtime, [])
  end
end
