defmodule EmergeDemo.State do
  @moduledoc false

  use Solve

  alias EmergeDemo.CounterController

  @impl Solve
  def controllers() do
    [controller!(name: :counter, module: CounterController)]
  end
end
