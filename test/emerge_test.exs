defmodule EmergeTest do
  use ExUnit.Case
  doctest Emerge

  test "greets the world" do
    assert Emerge.hello() == :world
  end
end
