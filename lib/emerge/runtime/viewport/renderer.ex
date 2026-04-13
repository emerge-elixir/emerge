defmodule Emerge.Runtime.Viewport.Renderer do
  @moduledoc false

  @type heartbeat_message :: {:emerge_viewport_renderer, :heartbeat}

  @callback start(keyword(), keyword()) :: {:ok, term()} | {:error, term()}
  @callback stop(term()) :: :ok
  @callback running?(term()) :: boolean()
  @doc """
  Renderers should send this message to the viewport process to report liveness.

  The viewport watchdog treats these heartbeats as the primary renderer health
  signal instead of polling `running?/1` during steady-state operation.
  """
  @spec heartbeat_message() :: heartbeat_message()
  def heartbeat_message, do: {:emerge_viewport_renderer, :heartbeat}

  @callback set_input_target(term(), pid() | nil) :: :ok
  @callback set_log_target(term(), pid() | nil) :: :ok
  @callback set_input_mask(term(), non_neg_integer()) :: :ok

  @callback upload_tree(term(), Emerge.Engine.Element.t()) ::
              {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}

  @callback patch_tree(term(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t()) ::
              {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
end
