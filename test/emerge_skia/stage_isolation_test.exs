# Opt-in: build the matched NIF with stage-probes and run this file with
# EMERGE_TEST_STAGES=headless (or raster/opengl/vulkan on a private test seat).
if System.get_env("EMERGE_TEST_STAGES") in ~w(headless raster opengl vulkan) do
  defmodule EmergeSkia.StageIsolationTest do
    use ExUnit.Case, async: false
    alias EmergeSkia.Native

    @api Map.fetch!(
           %{
             "headless" => :headless,
             "raster" => :raster,
             "opengl" => :opengl,
             "vulkan" => :vulkan
           },
           System.fetch_env!("EMERGE_TEST_STAGES")
         )

    defmodule View do
      use Emerge

      def mount(opts) do
        to = Keyword.fetch!(opts, :to)
        api = Keyword.fetch!(opts, :api)

        output =
          if api == :headless,
            do: %Emerge.Config.Headless{target: to, size: {64, 64}},
            else: %Emerge.Config.Window{
              backend: :wayland,
              title: "Owner isolation",
              size: {64, 64}
            }

        {:ok, %{to: to, pose: {:white, :initial}},
         %Emerge.Config{
           output: output,
           renderer: [api: if(api == :headless, do: :raster, else: api)],
           input: [events: [:pointer_button, :resize]],
           assets: [otp_app: :emerge]
         }}
      end

      def render(%{pose: {color, msg}}),
        do:
          el(
            [width(fill()), height(fill()), Background.color(color), Event.on_click(msg)],
            none()
          )

      def handle_input(e, s),
        do:
          (
            send(s.to, {:raw, e})
            {:noreply, s}
          )

      def handle_info({:pose, pose}, s), do: {:noreply, Viewport.rerender(%{s | pose: pose})}

      def handle_info(m, s),
        do:
          (
            send(s.to, {:routed, m})
            {:noreply, s}
          )
    end

    setup do
      v = start_supervised!({View, [api: @api, to: self()]})
      r = Emerge.renderer(v)
      selected = if @api == :headless, do: :raster, else: @api
      assert {:ok, %{rendering_api: %{selected: ^selected}}} = EmergeSkia.renderer_info(r)
      gate(r, :opportunity, false)
      send(v, {:pose, {:red, :a}})
      stage(:installed, 2)
      output(r, :red)
      %{v: v, r: r}
    end

    test "two computed command sets before a draw claim replay only the newer one", %{v: v, r: r} do
      gate(r, :commands, true)
      send(v, {:pose, {:green, :b}})
      stage(:computed, 3)
      send(v, {:pose, {:blue, :c}})
      c = stage(:computed, 4)
      click(r, :a)
      gate(r, :commands, false)
      assert stage(:drawing, 4) == c
      assert stage(:installed, 4) == c
      refute_received {:emerge_stage, :drawing, _, 3}
      output(r, :blue)
      click(r, :c)
    end

    test "two completed images before the opportunity install only the newer pixels and routes",
         %{v: v, r: r} do
      gate(r, :opportunity, true)
      send(v, {:pose, {:green, :b}})
      stage(:ready, 3)
      send(v, {:pose, {:blue, :c}})
      c = stage(:ready, 4)
      output(r, :red)
      click(r, :a)
      gate(r, :opportunity, false)
      assert stage(:installed, 4) == c
      refute_received {:emerge_stage, :installed, _, 3}
      output(r, :blue)
      click(r, :c)
    end

    test "B hands off while C is still drawing into a different target", %{v: v, r: r} do
      gate(r, :opportunity, true)
      send(v, {:pose, {:green, :b}})
      b = stage(:ready, 3)
      gate(r, :drawing, true)
      send(v, {:pose, {:blue, :c}})
      c = stage(:drawing, 4)
      gate(r, :opportunity, false)
      assert stage(:installed, 3) == b
      refute_received {:emerge_stage, :ready, _, 4}
      output(r, :green)
      click(r, :b)
      gate(r, :drawing, false)
      assert stage(:installed, 4) == c
      output(r, :blue)
      click(r, :c)
      # Change nothing but the BEAM listener. The older map cannot win.
      send(v, {:pose, {:blue, :d}})
      stage(:installed, 5)
      output(r, :blue)
      click(r, :d)
    end

    test "a press does not survive a skipped absence and remount of the same wire ID", %{r: r} do
      button(r, :press)
      assert_receive {:raw, {:pointer_button, {:left, :press, [], _}}}
      gate(r, :commands, true)

      assert :ok =
               Native.renderer_patch(
                 r,
                 Emerge.Engine.Patch.encode(Emerge.Engine.Patch.patch(1, nil))
               )

      stage(:computed, 3)

      {wire, _} =
        Emerge.Engine.DiffState.diff_and_encode_binary(
          Emerge.Engine.DiffState.new(),
          View.render(%{pose: {:blue, :a}})
        )

      assert :ok = Native.renderer_upload(r, wire)
      stage(:computed, 4)
      gate(r, :commands, false)
      stage(:installed, 4)
      output(r, :blue)
      button(r, :release)
      assert_receive {:raw, {:pointer_button, {:left, :release, [], _}}}
      refute_receive {:routed, {:a, _}}
      click(r, :a)
    end

    test "a definitely unsubmitted claim installs neither pixels nor callback routes", %{
      r: r,
      v: v
    } do
      gate(r, :claimed, true)
      send(v, {:pose, {:green, :b}})
      b = stage(:claimed, 3)
      send(v, {:pose, {:blue, :c}})
      stage(:ready, 4)
      click(r, :a)
      gate(r, :opportunity, true)
      gate(r, :reject, true)
      gate(r, :claimed, false)
      assert stage(:reject, 3) == b
      output(r, :red)
      click(r, :a)
      gate(r, :opportunity, false)
      stage(:installed, 4)
      refute_received {:emerge_stage, :installed, _, 3}
      output(r, :blue)
      click(r, :c)
    end

    test "delayed capture reads the installation it pinned, not a later frame", %{v: v, r: r} do
      gate(r, :pinned, true)
      read = Task.async(fn -> EmergeSkia.render_to_pixels(r, region: {5, 5, 1, 1}) end)
      stage(:pinned, 2)
      send(v, {:pose, {:blue, :c}})
      stage(:installed, 3)
      click(r, :c)
      gate(r, :pinned, false)
      assert {:ok, <<255, 0, 0, 255>>} = Task.await(read)
      output(r, :blue)
    end

    if @api == :headless do
      test "caller death in FEED and after a directory slice releases the writer and all private state",
           %{r: r, v: v} do
        {:ok, {session, 2}} = Native.renderer_revision_nif(r, nil)
        parent = self()
        wire = wide_replacement()
        {{doc, _}, {workspace, _}, {media, _}} = Native.renderer_storage_nif(r)

        for gate <- [:feed, :sliced] do
          gate(r, gate, true)

          {pid, mon} =
            spawn_monitor(fn -> Native.submit_revision(r, {session, 2}, wire, false, parent) end)

          stage(gate, 3)
          assert {:error, :busy} = Native.renderer_patch(r, wire)
          click(r, :a)
          pools_progress()
          Process.exit(pid, :kill)
          assert_receive {:DOWN, ^mon, :process, ^pid, :killed}
          assert_receive {:emerge_skia_credit, ^session}, 2_000
          assert {:ok, {^session, 2}} = Native.renderer_revision_nif(r, nil)

          {{used, doc_peak}, {^workspace, workspace_peak}, {^media, _}} =
            Native.renderer_storage_nif(r)

          assert used <= doc and doc_peak > doc
          if gate == :sliced, do: assert(workspace_peak > workspace)
          refute_received {:emerge_skia_credit, ^session}
          gate(r, gate, false)
        end

        send(v, {:pose, {:blue, :c}})
        stage(:installed, 3)
        output(r, :blue)
        click(r, :c)
      end

      test "expiry before commit and a lost reply after commit have distinct outcomes", %{r: r} do
        gate(r, :short_lease, true)
        gate(r, :sliced, true)
        pending = Task.async(fn -> Native.renderer_patch(r, wide_replacement()) end)
        stage(:sliced, 3)
        assert {:error, "edit ticket expired"} = Task.await(pending)
        assert {:ok, {session, 2}} = Native.renderer_revision_nif(r, nil)
        gate(r, :short_lease, false)
        gate(r, :sliced, false)
        gate(r, :applied, true)

        {pid, mon} =
          spawn_monitor(fn ->
            Native.renderer_patch(r, Emerge.Engine.Patch.encode(Emerge.Engine.Patch.patch(1, 1)))
          end)

        stage(:applied, 3)
        Process.exit(pid, :kill)
        assert_receive {:DOWN, ^mon, :process, ^pid, :killed}
        assert {:ok, {^session, 3}} = Native.renderer_revision_nif(r, nil)
        gate(r, :applied, false)
        stage(:installed, 3)
        output(r, :red)
        click(r, :a)
      end

      test "stop cancels an incomplete draft and a restarted session rejects its base", %{r: r} do
        {:ok, old} = Native.renderer_revision_nif(r, nil)
        gate(r, :sliced, true)
        pending = Task.async(fn -> Native.renderer_patch(r, wide_replacement()) end)
        stage(:sliced, 3)
        assert :ok = EmergeSkia.stop(r)
        assert {:error, "renderer is stopping"} = Task.await(pending)

        assert {:ok, next} =
                 EmergeSkia.start(%Emerge.Config{
                   output: %Emerge.Config.Headless{target: self(), size: {4, 4}},
                   renderer: [api: :raster],
                   assets: [otp_app: :emerge]
                 })

        on_exit(fn -> EmergeSkia.stop(next) end)
        assert {:error, :stale_base} = Native.submit_revision(next, old, <<>>, false, nil)
        assert {:ok, {_, 0}} = Native.renderer_revision_nif(next, nil)
      end

      test "a stalled layout accepts two successors; full refuses before inspecting wire", %{
        r: r,
        v: v
      } do
        gate(r, :layout, true)
        send(v, {:pose, {:green, :b}})
        stage(:layout, 3)

        for {color, msg, revision} <- [{:red, :c, 4}, {:blue, :d, 5}] do
          send(v, {:pose, {color, msg}})
          stage(:applied, revision)
        end

        assert {:ok, {session, 5}} = Native.renderer_revision_nif(r, nil)
        # Malformed, large and bit-offset. It must not get as far as decoding.
        bad = unaligned(:binary.copy(<<255>>, 4 * 1024 * 1024))
        assert {:error, :busy} = Native.renderer_patch(r, bad)
        assert {:error, :stale_base} = Native.submit_revision(r, {session, 4}, bad, false, nil)
        send(v, {:pose, {:green, :e}})
        output(r, :red)
        click(r, :a)
        gate(r, :layout, false)
        stage(:installed, 6)
        output(r, :green)
        click(r, :e)
      end

      test "an abandoned private draft cannot publish; prior layout and draw still progress", %{
        r: r,
        v: v
      } do
        gate(r, :commands, true)
        send(v, {:pose, {:green, :b}})
        stage(:computed, 3)
        gate(r, :writer, true)
        {writer, mon} = spawn_monitor(fn -> Native.submit_revision(r, nil, <<>>, true, nil) end)
        stage(:writer, 4)
        assert {:error, :busy} = Native.renderer_patch(r, <<255>>)
        gate(r, :commands, false)
        stage(:installed, 3)
        output(r, :green)
        click(r, :b)
        Process.exit(writer, :kill)
        assert_receive {:DOWN, ^mon, :process, ^writer, :killed}
        assert {:ok, {session, 3}} = Native.renderer_revision_nif(r, nil)
        send(v, {:pose, {:blue, :c}})
        gate(r, :writer, false)
        stage(:installed, 4)
        output(r, :blue)
        click(r, :c)
        # Once admitted, the same malformed but bit-offset input fails cleanly.
        assert {:error, why} = Native.renderer_patch(r, unaligned(<<255::size(8 * 5000)>>))
        assert is_binary(why)
        assert {:ok, {^session, 4}} = Native.renderer_revision_nif(r, nil)
      end

      defp unaligned(bytes) do
        size = byte_size(bytes)
        bits = <<1::1, bytes::binary, 0::7>>
        <<_::1, result::binary-size(^size), _::7>> = bits
        result
      end

      test "a claimed frame is immutable and neither dirty pool waits for its delivery", %{
        v: v,
        r: r
      } do
        gate(r, :claimed, true)
        send(v, {:pose, {:green, :b}})
        b = stage(:claimed, 3)
        send(v, {:pose, {:blue, :c}})
        c = stage(:ready, 4)
        output(r, :red)
        click(r, :a)
        pools_progress()
        assert :ok == EmergeSkia.set_log_target(r, nil)
        gate(r, :claimed, false)
        assert stage(:installed, 3) == b
        assert stage(:installed, 4) == c
        output(r, :blue)
      end
    end

    defp wide_replacement do
      children = for id <- 2000..2299, do: {:child, {:node, id, :none, %{}, []}}
      Emerge.Engine.Patch.encode(Emerge.Engine.Patch.patch(1, {:node, 1000, :row, %{}, children}))
    end

    defp pools_progress do
      wire = <<"EMRG", 10, 0::64, 0::64, 0::32>>
      cpu = Task.async(fn -> Native.tree_roundtrip(wire) end)

      io =
        Task.async(fn ->
          :prim_file.read_file(Application.app_dir(:emerge, "priv/test_assets/alpha_grid.png"))
        end)

      Process.send_after(self(), :beat, 0)
      assert_receive :beat
      assert {:ok, ^wire} = Task.await(cpu)
      assert {:ok, <<137, "PNG", _::binary>>} = Task.await(io)
    end

    defp gate(r, s, hold), do: assert(:ok == Native.renderer_stage_nif(r, s, hold))

    defp stage(s, revision) do
      assert_receive {:emerge_stage, ^s, serial, ^revision}, 5_000
      serial
    end

    defp output(r, color) do
      pixel =
        %{red: <<255, 0, 0, 255>>, green: <<0, 255, 0, 255>>, blue: <<0, 0, 255, 255>>}[color]

      assert {:ok, ^pixel} = EmergeSkia.render_to_pixels(r, region: {5, 5, 1, 1})
    end

    if @api == :headless do
      defp button(r, phase),
        do:
          assert(
            :ok == EmergeSkia.send_input(r, {:pointer_button, {:left, phase, [], {5.0, 5.0}}})
          )
    else
      test "four held target leases stop a fifth draw, not edits, input or capture", %{r: r, v: v} do
        gate(r, :retain_targets, true)

        for {n, color} <- Enum.zip(3..6, [:green, :blue, :red, :green]) do
          send(v, {:pose, {color, n}})
          id = stage(:retain_targets, n)
          assert stage(:installed, n) == id
        end

        send(v, {:pose, {:blue, 7}})
        c = stage(:computed, 7)
        click(r, 6)
        output(r, :green)
        pools_progress()
        refute_received {:emerge_stage, :ready, _, 7}
        gate(r, :retain_targets, false)
        assert stage(:installed, 7) == c
        output(r, :blue)
        click(r, 7)
      end

      test "an uncertain reported handoff stops the output instead of installing or retrying it",
           %{r: r, v: v} do
        gate(r, :uncertain, true)
        send(v, {:pose, {:blue, :c}})
        c = stage(:uncertain, 3)
        assert :ok = EmergeSkia.stop(r)

        assert {:ok, %{cleanup_complete: true, failure: %{reason: "uncertain handoff (probe)"}}} =
                 EmergeSkia.renderer_status(r)

        refute_received {:emerge_stage, :installed, ^c, 3}
      end

      test "a fractional configure invalidates an unfinished proof and unstarted handoff, not the installed image",
           %{r: r, v: v} do
        sway(~s([title="^Owner isolation$"] floating enable, resize set 101 75))
        assert_receive {:raw, {:resize, {101, 75, 1.0}}}, 2_000
        wait_size(r, 101, 75)
        gate(r, :claimed, true)
        send(v, {:pose, {:green, :b}})
        stage(:claimed, 3)
        gate(r, :opportunity, true)
        gate(r, :sliced, true)
        pending = Task.async(fn -> Native.renderer_patch(r, wide_replacement()) end)
        stage(:sliced, 4)
        on_exit(fn -> sway("output HEADLESS-1 scale 1") end)
        sway("output HEADLESS-1 scale 1.5")
        assert_receive {:raw, {:resize, {152, 113, 1.5}}}, 3_000
        assert {:error, :busy} = Task.await(pending)
        assert {:ok, {_, 3}} = Native.renderer_revision_nif(r, nil)
        gate(r, :sliced, false)
        gate(r, :claimed, false)
        assert {:ok, pixels} = EmergeSkia.render_to_pixels(r)
        assert byte_size(pixels) == 101 * 75 * 4
        output(r, :red)
        point = fraction_pointer()
        {ax, ay} = mapped_click(point, :a, 1.0)
        send(v, {:pose, {:blue, :c}})
        stage(:ready, 4)
        gate(r, :opportunity, false)
        stage(:installed, 4)
        wait_size(r, 152, 113)
        output(r, :blue)
        {cx, cy} = mapped_click(point, :c, 1.5)
        assert_in_delta cx, ax * 152 / 101, 0.002
        assert_in_delta cy, ay * 113 / 75, 0.002
        pools_progress()
      end

      if @api in [:opengl, :vulkan] do
        test "a successful handoff reported after configure installs its own pixels, mapping and routes",
             %{r: r, v: v} do
          sway(~s([title="^Owner isolation$"] floating enable, resize set 101 75))
          assert_receive {:raw, {:resize, {101, 75, 1.0}}}, 2_000
          wait_size(r, 101, 75)
          gate(r, :handed, true)
          send(v, {:pose, {:green, :b}})
          b = stage(:handed, 3)
          gate(r, :opportunity, true)
          on_exit(fn -> sway("output HEADLESS-1 scale 1") end)
          sway("output HEADLESS-1 scale 1.5")
          assert_receive {:raw, {:resize, {152, 113, 1.5}}}, 3_000
          send(v, {:pose, {:blue, :c}})
          stage(:computed, 4)
          point = fraction_pointer()
          {ax, ay} = mapped_click(point, :a, 1.0)
          gate(r, :handed, false)
          assert stage(:installed, 3) == b
          wait_size(r, 101, 75)
          output(r, :green)
          assert {^ax, ^ay} = mapped_click(point, :b, 1.0)
          gate(r, :opportunity, false)
          stage(:installed, 4)
          wait_size(r, 152, 113)
          output(r, :blue)
          {cx, cy} = mapped_click(point, :c, 1.5)
          assert_in_delta cx, ax * 152 / 101, 0.002
          assert_in_delta cy, ay * 113 / 75, 0.002
        end
      end

      # The isolated virtual pointer speaks normalized 720x500, not logical
      # surface units. Reuse exactly the same physical point on both installs.
      defp fraction_pointer do
        %{"x" => x, "y" => y} = box(sway_query("get_tree"))

        [%{"rect" => %{"width" => w, "height" => h}}] =
          Enum.filter(sway_query("get_outputs"), &(&1["name"] == "HEADLESS-1"))

        {round((x + 45) * 720 / w), round((y + 35) * 500 / h)}
      end

      defp mapped_click({x, y}, msg, scale) do
        for {action, bit} <- [press: 1, release: 0] do
          File.write!(System.fetch_env!("EMERGE_STAGE_POINTER"), "#{x} #{y} #{bit}\n")
          assert_receive {:raw, {:pointer_button, {:left, ^action, [], {px, py}}}}, 2_000
          {px, py}
        end
        |> List.last()
        |> then(fn {px, py} ->
          assert_receive {:routed, {^msg, %{x: lx, y: ly}}}, 2_000
          assert_in_delta lx, px / scale, 0.002
          assert_in_delta ly, py / scale, 0.002
          {px, py}
        end)
      end

      defp sway_query(key) do
        assert System.fetch_env!("CARDS_TEST_DESKTOP") == System.fetch_env!("XDG_RUNTIME_DIR")

        {json, 0} =
          System.cmd(Path.join(System.fetch_env!("TOOLS"), "swaymsg"), ["-r", "-t", key])

        :json.decode(json)
      end

      defp wait_size(r, w, h, tries \\ 150)
      defp wait_size(_r, w, h, 0), do: flunk("no installation at #{w}x#{h}")

      defp wait_size(r, w, h, n) do
        case EmergeSkia.render_to_pixels(r) do
          {:ok, p} when byte_size(p) == w * h * 4 ->
            :ok

          _ ->
            Process.sleep(10)
            wait_size(r, w, h, n - 1)
        end
      end

      defp sway(command) do
        assert System.fetch_env!("CARDS_TEST_DESKTOP") == System.fetch_env!("XDG_RUNTIME_DIR")
        {json, 0} = System.cmd(Path.join(System.fetch_env!("TOOLS"), "swaymsg"), ["-r", command])
        assert Enum.all?(:json.decode(json), & &1["success"])
      end

      # Only the explicitly supplied, isolated Wayland virtual pointer may
      # generate input. Never drive the user's desktop.
      defp button(_r, phase) do
        %{"x" => x, "y" => y} = box(sway_query("get_tree"))

        File.write!(
          System.fetch_env!("EMERGE_STAGE_POINTER"),
          "#{x + 5} #{y + 5} #{if phase == :press, do: 1, else: 0}\n"
        )
      end

      defp box(%{"name" => "Owner isolation", "rect" => rect}), do: rect
      defp box(n), do: Enum.find_value(n["nodes"] ++ n["floating_nodes"], &box/1)
    end

    defp click(r, msg) do
      for phase <- [:press, :release] do
        button(r, phase)
        assert_receive {:raw, {:pointer_button, {:left, ^phase, [], {5.0, 5.0}}}}, 2_000
      end

      assert_receive {:routed, {^msg, %{x: 5.0, y: 5.0}}}, 2_000
    end
  end
end
