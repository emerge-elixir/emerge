defmodule Emerge.Viewport.ReloadGroup do
  @moduledoc false

  @scope __MODULE__
  @group :viewports

  @spec ensure_started() :: :ok
  def ensure_started do
    case :pg.start(@scope) do
      {:ok, _pid} -> :ok
      {:error, {:already_started, _pid}} -> :ok
    end
  end

  @spec join(pid()) :: :ok
  def join(pid) when is_pid(pid) do
    ensure_started()
    :pg.join(@scope, @group, pid)
  end

  @spec leave(pid()) :: :ok
  def leave(pid) when is_pid(pid) do
    ensure_started()

    case :pg.leave(@scope, @group, pid) do
      :ok -> :ok
      :not_joined -> :ok
    end
  end

  @spec local_members() :: [pid()]
  def local_members do
    ensure_started()
    :pg.get_local_members(@scope, @group)
  end

  @spec broadcast(term()) :: :ok
  def broadcast(message) do
    local_members()
    |> Enum.each(&send(&1, message))

    :ok
  end
end
