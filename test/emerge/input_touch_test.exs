defmodule Emerge.InputTouchTest do
  use ExUnit.Case, async: true
  alias Emerge.Config

  test "touch is an explicit observer subscription, independent of mouse subscriptions" do
    config = Config.Normalize.config!(%Config{input: [events: [:touch]]})
    assert config.input.events == [:touch]
    assert Config.Native.input_mask(config.input) == 0x100

    assert Config.Native.input_mask(%Config.Input{events: [:pointer_move, :pointer_button]}) ==
             0x0C

    assert Config.Native.input_mask(%Config.Input{events: Config.Input.all_events()}) == 0x1FF
  end

  test "touch phases retain contact identity and position without mouse-action translation" do
    for phase <- [:down, :move, :up, :cancel], id <- [1, 2, 0x1_0000_0000] do
      event = {:touch, {id, phase, {10.5, 20.0}}}
      assert Emerge.Input.from_native(event) == event
    end
  end
end
