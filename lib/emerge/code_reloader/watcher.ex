defmodule Emerge.CodeReloader.Watcher do
  @moduledoc false

  @callback start_link(keyword()) :: GenServer.on_start()
end
