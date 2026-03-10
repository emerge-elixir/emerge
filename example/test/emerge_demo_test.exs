defmodule EmergeDemoTest do
  use ExUnit.Case

  alias EmergeDemo.UI
  alias EmergeSkia.VideoTarget

  test "runtime config is available" do
    config = EmergeDemo.runtime_config()

    assert Keyword.has_key?(config, :renderer)
    assert Keyword.has_key?(config, :video)
    assert Keyword.has_key?(config, :pipeline)
  end

  test "build_tree returns a root element with video content" do
    target = %VideoTarget{id: "preview", width: 1920, height: 1080, mode: :prime, ref: make_ref()}

    tree =
      UI.build_tree(
        target,
        %{status: :waiting_for_frames, last_error: nil, stream_format: nil},
        EmergeDemo.runtime_config()
      )

    assert tree.type == :el
    assert hd(tree.children).type == :video
    assert Map.has_key?(tree.attrs, :in_front)
  end
end
