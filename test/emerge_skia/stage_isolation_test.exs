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

      def render(%{pose: {:image, angle, msg}}) do
        el(
          [width(px(64)), height(px(64)), Background.color(:white), Event.on_click(:host)],
          el(
            [center_x(), center_y(), width(px(32)), height(px(32)), Border.rounded(2)],
            svg(
              [width(px(32)), height(px(32)), Transform.rotate(angle), Event.on_click(msg)],
              "test_assets/single_axis_square.svg"
            )
          )
        )
      end

      def render(%{pose: {:card, mode, msg}}) do
        turn = if mode == :layout, do: rotate(90), else: Transform.rotate(90)

        row([width(px(64)), height(px(64)), Background.color(:white)], [
          el(
            [width(px(40)), height(px(16)), turn],
            el(
              [
                width(fill()),
                height(fill()),
                Transform.scale(-1),
                Background.color(:red),
                Event.on_click(msg)
              ],
              none()
            )
          ),
          el(
            [width(px(8)), height(px(8)), Background.color(:blue), Event.on_click(:marker)],
            none()
          )
        ])
      end

      def render(%{pose: {:scroll, pos, route}}) do
        column(
          [width(px(64)), height(px(64)), scrollbar_y(), scroll_y(pos), Event.on_scroll(route)],
          Enum.map([:red, :blue], fn c ->
            el([width(px(64)), height(px(64)), Background.color(c), Event.on_click(c)], none())
          end)
        )
      end

      def render(%{pose: {:packed_menu, sibling}}) do
        menu =
          el(
            [width(fill()), height(px(10)), Background.color(:green), Event.on_click(:menu)],
            none()
          )

        card = el([width(px(12)), height(px(20))], none())

        column([width(px(64)), height(px(64)), Background.color(:blue), Event.on_click(:paper)], [
          wrapped_column(
            [width(content()), height(fill()), spacing_xy(6, 4), Nearby.in_front(menu)],
            [card, card]
          ),
          el([width(px(8)), height(px(sibling))], none())
        ])
      end

      def render(%{pose: {:pack, words, msg}}) do
        row([width(fill()), height(fill())], [
          wrapped_column([width(content()), height(px(60)), spacing_xy(6, 4)], [
            paragraph([width(px(48))], [text(words)]),
            el(
              [width(px(20)), height(px(20)), Background.color(:red), Event.on_click(msg)],
              none()
            )
          ]),
          el(
            [width(fill()), height(px(8)), Background.color(:blue), Event.on_click(:peer)],
            none()
          )
        ])
      end

      def render(%{pose: {:row, h, msg}}) do
        row([width(px(64)), height(px(h))], [
          image([height(fill()), Event.on_click(msg)], "test_assets/single_axis_wide.png"),
          el(
            [width(px(8)), height(px(8)), Background.color(:blue), Event.on_click(:next)],
            none()
          )
        ])
      end

      def render(%{pose: {:share, h, msg}}) do
        column([height(px(48))], [
          image(
            [height(min(content(), fill())), Event.on_click(msg)],
            "test_assets/single_axis_wide.png"
          ),
          el(
            [width(px(64)), height(px(h)), Background.color(:blue), Event.on_click(:next)],
            none()
          )
        ])
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

    test "a held then rejected image turn keeps the installed pixels, inverse and callback together",
         %{
           v: v,
           r: r
         } do
      point = {32.0, 13.0}
      send(v, {:pose, {:image, 0, :old_art}})
      stage(:installed, 3)
      # Readiness may install a second frame of the same revision.
      await_pixel(r, 3, {32, 32, 1, 1}, <<255, 0, 0, 255>>)
      gate(r, :opportunity, true)
      send(v, {:pose, {:image, 45, :new_art}})
      stage(:ready, 4)

      old = fn ->
        assert {:ok, <<255, 255, 255, 255>>} =
                 EmergeSkia.render_to_pixels(r, region: {32, 13, 1, 1})

        click_point(r, point)
        assert_receive {:routed, {:host, %{x: 32.0, y: 13.0}}}, 2000
      end

      old.()
      gate(r, :claimed, true)
      gate(r, :opportunity, false)
      claim = stage(:claimed, 4)
      gate(r, :opportunity, true)
      gate(r, :reject, true)
      gate(r, :claimed, false)
      assert stage(:reject, 4) == claim
      old.()
      gate(r, :opportunity, false)
      stage(:installed, 4)
      assert {:ok, <<255, 0, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: {32, 13, 1, 1})

      click_point(r, point)
      assert_receive {:routed, {:new_art, %{x: lx, y: ly}}}, 2000
      assert_in_delta lx, 16 - 19 / :math.sqrt(2), 0.002
      assert_in_delta ly, lx, 0.002
    end

    test "a refused layout turn cannot install its nested inverse or callback", %{v: v, r: r} do
      point = {20.0, 4.0}
      region = {20, 4, 1, 1}
      send(v, {:pose, {:card, :paint, :old}})
      stage(:installed, 3)
      gate(r, :opportunity, true)
      send(v, {:pose, {:card, :layout, :turned}})
      stage(:ready, 4)

      old = fn ->
        assert {:ok, <<255, 0, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
        click_point(r, point)
        assert_receive {:routed, {:old, %{x: 24.0, y: 8.0}}}, 2000
      end

      old.()
      gate(r, :claimed, true)
      gate(r, :opportunity, false)
      claim = stage(:claimed, 4)
      gate(r, :opportunity, true)
      gate(r, :reject, true)
      gate(r, :claimed, false)
      assert stage(:reject, 4) == claim
      old.()
      gate(r, :opportunity, false)
      stage(:installed, 4)
      assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
      click_point(r, point)
      assert_receive {:routed, {:marker, %{x: 4.0, y: 4.0}}}, 2000
      click_point(r, {8.0, 36.0})
      assert_receive {:routed, {:turned, %{x: 4.0, y: 8.0}}}, 2000
      # A callback-only edit still waits for its own installation.
      gate(r, :opportunity, true)
      send(v, {:pose, {:card, :layout, :rerouted}})
      stage(:ready, 5)
      click_point(r, {8.0, 36.0})
      assert_receive {:routed, {:turned, %{x: 4.0, y: 8.0}}}, 2000
      gate(r, :opportunity, false)
      stage(:installed, 5)
      click_point(r, {8.0, 36.0})
      assert_receive {:routed, {:rerouted, %{x: 4.0, y: 8.0}}}, 2000
    end

    test "rejected paragraph packing cannot publish its new card or displace the installed peer",
         %{v: v, r: r} do
      region = {57, 3, 1, 1}
      send(v, {:pose, {:pack, "AA BB", :short}})
      stage(:installed, 3)
      gate(r, :opportunity, true)
      send(v, {:pose, {:pack, "AA BB CC DD EE", :long}})
      stage(:ready, 4)

      old = fn ->
        assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
        click_point(r, {57.0, 3.0})
        assert_receive {:routed, {:peer, %{x: 9.0, y: 3.0}}}, 2000
      end

      old.()
      gate(r, :claimed, true)
      gate(r, :opportunity, false)
      claim = stage(:claimed, 4)
      gate(r, :opportunity, true)
      gate(r, :reject, true)
      gate(r, :claimed, false)
      assert stage(:reject, 4) == claim
      old.()
      gate(r, :opportunity, false)
      stage(:installed, 4)
      assert {:ok, <<255, 0, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
      click_point(r, {57.0, 3.0})
      assert_receive {:routed, {:long, %{x: 3.0, y: 3.0}}}, 2000
    end

    test "a ready height edit does not move an installed image or its sibling", %{v: v, r: r} do
      region = {34, 3, 1, 1}
      send(v, {:pose, {:row, 16, :old_image}})
      stage(:installed, 3)
      await_pixel(r, 3, region, <<0, 0, 255, 255>>)
      gate(r, :opportunity, true)
      send(v, {:pose, {:row, 24, :new_image}})
      stage(:ready, 4)
      assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
      click_point(r, {34.0, 3.0})
      assert_receive {:routed, {:next, %{x: 2.0, y: 3.0}}}, 2000
      gate(r, :opportunity, false)
      stage(:installed, 4)
      assert {:ok, <<255, 0, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
      click_point(r, {34.0, 3.0})
      assert_receive {:routed, {:new_image, %{x: 34.0, y: 3.0}}}, 2000
    end

    test "a rejected bounded Column share retains the installed sibling and its local click", %{
      v: v,
      r: r
    } do
      region = {30, 36, 1, 1}
      send(v, {:pose, {:share, 16, :old}})
      stage(:installed, 3)
      await_pixel(r, 3, region, <<0, 0, 255, 255>>)
      gate(r, :opportunity, true)
      send(v, {:pose, {:share, 8, :grown}})
      stage(:ready, 4)

      old = fn ->
        assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
        click_point(r, {30.0, 36.0})
        assert_receive {:routed, {:next, %{x: 30.0, y: 4.0}}}, 2000
      end

      old.()
      gate(r, :claimed, true)
      gate(r, :opportunity, false)
      claim = stage(:claimed, 4)
      gate(r, :opportunity, true)
      gate(r, :reject, true)
      gate(r, :claimed, false)
      assert stage(:reject, 4) == claim
      old.()
      gate(r, :opportunity, false)
      stage(:installed, 4)
      assert {:ok, <<255, 0, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: region)
      click_point(r, {30.0, 36.0})
      assert_receive {:routed, {:grown, %{x: 30.0, y: 36.0}}}, 2000
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

    defp await_pixel(r, revision, region, pixel) do
      unless EmergeSkia.render_to_pixels(r, region: region) == {:ok, pixel} do
        stage(:installed, revision)
        await_pixel(r, revision, region, pixel)
      end
    end

    defp output(r, color) do
      pixel =
        %{red: <<255, 0, 0, 255>>, green: <<0, 255, 0, 255>>, blue: <<0, 0, 255, 255>>}[color]

      assert {:ok, ^pixel} = EmergeSkia.render_to_pixels(r, region: {5, 5, 1, 1})
    end

    if @api == :headless do
      test "a packed host and its fill-width menu change installed pixels and hits together", %{
        v: v,
        r: r
      } do
        send(v, {:pose, {:packed_menu, 20}})
        stage(:installed, 3)
        gate(r, :opportunity, true)
        send(v, {:pose, {:packed_menu, 21}})
        stage(:ready, 4)

        old = fn ->
          assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: {25, 5, 1, 1})
          click_point(r, {25.0, 5.0})
          assert_receive {:routed, {:paper, %{x: 25.0, y: 5.0}}}
        end

        old.()
        gate(r, :claimed, true)
        gate(r, :opportunity, false)
        claim = stage(:claimed, 4)
        gate(r, :opportunity, true)
        gate(r, :reject, true)
        gate(r, :claimed, false)
        assert stage(:reject, 4) == claim
        old.()
        gate(r, :opportunity, false)
        stage(:installed, 4)
        assert {:ok, <<0, 255, 0, 255>>} = EmergeSkia.render_to_pixels(r, region: {25, 5, 1, 1})
        click_point(r, {25.0, 5.0})
        assert_receive {:routed, {:menu, %{x: 25.0, y: 5.0}}}
      end

      test "wheel bursts use the installed scroll route; held or rejected app jumps cannot replay older input",
           %{v: v, r: r} do
        wheel = fn ->
          assert :ok == EmergeSkia.send_input(r, {:pointer_scroll, {{0, -16}, {5, 5}}})
        end

        send(v, {:pose, {:scroll, 0.0, :old}})
        stage(:installed, 3)
        gate(r, :commands, true)
        wheel.()
        assert_receive {:routed, {:old, %{y: 0.25}}}
        wheel.()
        assert_receive {:routed, {:old, %{y: 0.5}}}
        send(v, {:pose, {:scroll, 0.25, :echo}})
        stage(:computed, 4)
        output(r, :red)
        gate(r, :commands, false)
        stage(:installed, 4)
        assert {:ok, <<0, 0, 255, 255>>} = EmergeSkia.render_to_pixels(r, region: {5, 40, 1, 1})
        gate(r, :opportunity, true)
        # A non-echo is an app replacement. Hold it after drawing, while the
        # display still owns the older attachment, geometry and callback route.
        send(v, {:pose, {:scroll, 1.0, :new}})
        stage(:ready, 5)
        wheel.()
        assert_receive {:routed, {:echo, %{y: 0.75}}}
        gate(r, :claimed, true)
        gate(r, :opportunity, false)
        claim = stage(:claimed, 5)
        gate(r, :opportunity, true)
        gate(r, :reject, true)
        gate(r, :claimed, false)
        assert stage(:reject, 5) == claim
        output(r, :red)
        click_point(r, {5.0, 5.0})
        assert_receive {:routed, {:red, %{y: 37.0}}}
        gate(r, :opportunity, false)
        stage(:installed, 5)
        output(r, :blue)
        click_point(r, {5.0, 5.0})
        assert_receive {:routed, {:blue, %{y: 5.0}}}
        wheel.()
        refute_receive {:routed, {:new, _}}, 20
        assert :ok == EmergeSkia.send_input(r, {:pointer_scroll, {{0, 16}, {5, 5}}})
        assert_receive {:routed, {:new, %{y: 0.75}}}
      end

      defp button_at(r, phase, point),
        do: assert(:ok == EmergeSkia.send_input(r, {:pointer_button, {:left, phase, [], point}}))
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
      defp button_at(_r, phase, {px, py}) do
        %{"x" => x, "y" => y} = box(sway_query("get_tree"))

        File.write!(
          System.fetch_env!("EMERGE_STAGE_POINTER"),
          "#{x + round(px)} #{y + round(py)} #{if phase == :press, do: 1, else: 0}\n"
        )
      end

      defp box(%{"name" => "Owner isolation", "rect" => rect}), do: rect
      defp box(n), do: Enum.find_value(n["nodes"] ++ n["floating_nodes"], &box/1)
    end

    defp button(r, phase), do: button_at(r, phase, {5.0, 5.0})

    defp click_point(r, point) do
      for phase <- [:press, :release] do
        button_at(r, phase, point)
        assert_receive {:raw, {:pointer_button, {:left, ^phase, [], ^point}}}, 2_000
      end
    end

    defp click(r, msg) do
      click_point(r, {5.0, 5.0})
      assert_receive {:routed, {^msg, %{x: 5.0, y: 5.0}}}, 2_000
    end
  end
end
