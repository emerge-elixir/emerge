defmodule EmergeDemoTest do
  use ExUnit.Case
  doctest EmergeDemo

  test "greets the world" do
    assert EmergeDemo.hello() == :world
  end
end
