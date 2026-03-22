defmodule Emerge.Runtime.Viewport.Renderer do
  @moduledoc false

  @callback start(keyword(), keyword()) :: {:ok, term()} | {:error, term()}
  @callback stop(term()) :: :ok
  @callback running?(term()) :: boolean()
  @callback set_input_target(term(), pid() | nil) :: :ok
  @callback set_input_mask(term(), non_neg_integer()) :: :ok

  @callback upload_tree(term(), Emerge.Engine.Element.t()) ::
              {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}

  @callback patch_tree(term(), Emerge.Engine.diff_state(), Emerge.Engine.Element.t()) ::
              {Emerge.Engine.diff_state(), Emerge.Engine.Element.t()}
end
