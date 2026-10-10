defmodule Emerge.Engine.ScrollTarget do
  @moduledoc false
  # Keep semantic declaration identity separate from its current numeric mount.
  # Only declaration/id cross the native boundary, never arbitrary key terms.
  @enforce_keys [:key, :declaration, :id]
  defstruct [:key, :declaration, :id]
end
