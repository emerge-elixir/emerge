Code.require_file("../mix/release_checks.exs", __DIR__)

defmodule Emerge.ReleaseChecksTest do
  use ExUnit.Case, async: true
  alias Emerge.Mix.ReleaseChecks

  @root Path.expand("..", __DIR__)

  setup do
    directory =
      Path.join(System.tmp_dir!(), "emerge-release-checks-#{System.unique_integer([:positive])}")

    File.mkdir_p!(directory)
    on_exit(fn -> File.rm_rf!(directory) end)
    %{directory: directory}
  end

  test "current release manifests, lockfile, changelog and documentation agree" do
    version = Mix.Project.config()[:version]
    tag = "v#{version}"
    notes = ReleaseChecks.release_notes_from_files!(tag, @root)
    assert notes != ""

    lock = File.read!(Path.join(@root, "native/emerge_skia/Cargo.lock"))

    assert [version] ==
             Regex.run(
               ~r/\[\[package\]\]\nname = "emerge_skia"\nversion = "([^"]+)"/,
               lock,
               capture: :all_but_first
             )

    docs = Emerge.Mix.Docs.config(@root, version, Mix.Project.config()[:source_url])
    assert docs[:source_ref] == tag
    parsed = Version.parse!(version)
    assert "guides/migrations/#{parsed.major}.#{parsed.minor}.md" in docs[:extras]
  end

  test "Rust video interop dependencies meet the Elixir package's minimum version" do
    {:video_interop, requirement} =
      Enum.find(Mix.Project.config()[:deps], &(elem(&1, 0) == :video_interop))

    lock = File.read!(Path.join(@root, "native/emerge_skia/Cargo.lock"))

    [locked] =
      Regex.run(~r/\[\[package\]\]\nname = "video-interop"\nversion = "([^"]+)"/, lock,
        capture: :all_but_first
      )

    assert Version.match?(locked, requirement)
    manifest = File.read!(Path.join(@root, "native/emerge_skia/Cargo.toml"))

    minimums =
      Regex.scan(~r/^video-interop = (?:\{ version = )?"([^"]+)"/m, manifest,
        capture: :all_but_first
      )

    assert length(minimums) == 2
    for [minimum] <- minimums, do: assert(Version.match?(minimum, requirement))
  end

  test "NIF and macOS host artifact URLs use the current project release" do
    version = Mix.Project.config()[:version]
    base = "#{Mix.Project.config()[:source_url]}/releases/download/v#{version}"

    for target <- EmergeSkia.BuildConfig.precompiled_targets() do
      name = "libemerge_skia-v#{version}-nif-2.15-#{target}.so.tar.gz"
      assert EmergeSkia.BuildConfig.precompiled_tar_gz_url(name, %{}) == "#{base}/#{name}"
    end

    for target <- EmergeSkia.BuildConfig.macos_host_targets() do
      name = "macos_host-v#{version}-#{target}.tar.gz"
      assert EmergeSkia.BuildConfig.macos_host_archive_name(target) == name
      assert EmergeSkia.BuildConfig.macos_host_download_url(target, %{}) == "#{base}/#{name}"

      assert EmergeSkia.BuildConfig.macos_host_checksum_url(target, %{}) ==
               "#{base}/#{name}.sha256"
    end
  end

  test "extracts only the selected release" do
    changelog = """
    # Changelog
    ## [Unreleased]
    - Later.
    ## [0.4.0] - 2026-09-28
    ### Added

    - A feature.

    ## [0.3.4] - 2026-07-31
    - An older fix.
    """

    assert notes(changelog) == "### Added\n\n- A feature."
  end

  test "rejects unfinished or ambiguous release notes" do
    for changelog <- [
          "## [0.4.0] - Unreleased\n- A feature.",
          "## [0.4.0] - 2026-09-28\nDraft release notes; confirm the date.",
          "## [0.4.0] - 2026-09-28\n",
          "## [0.4.0] - 2026-09-28\n- A.\n## [0.4.0] - 2026-09-28\n- B.",
          "## [0.4.0] - 2026-02-30\n- A."
        ] do
      assert_raise ArgumentError, fn -> notes(changelog) end
    end
  end

  test "rejects tag and manifest mismatches" do
    changelog = "## [0.4.0] - 2026-09-28\n- A feature."

    for {tag, cargo} <- [{"main", "0.4.0"}, {"v0.4.1", "0.4.0"}, {"v0.4.0", "0.3.4"}] do
      assert_raise ArgumentError, fn ->
        ReleaseChecks.release_notes!(tag, changelog, "0.4.0", cargo)
      end
    end
  end

  test "reads literal package versions without evaluating Mix code", %{directory: root} do
    File.mkdir_p!(Path.join(root, "native/emerge_skia"))
    File.write!(Path.join(root, "mix.exs"), "  @version \"0.4.0\"\nraise \"must not run\"\n")
    File.write!(Path.join(root, "CHANGELOG.md"), "## [0.4.0] - 2026-09-28\n- Ready.\n")
    cargo = Path.join(root, "native/emerge_skia/Cargo.toml")

    File.write!(
      cargo,
      "[package]\nname = \"emerge_skia\"\nversion = \"0.4.0\"\n[dependencies]\nversion = \"wrong\"\n"
    )

    assert ReleaseChecks.release_notes_from_files!("v0.4.0", root) == "- Ready."
    File.write!(cargo, "[dependencies]\nversion = \"0.4.0\"\n")
    assert_raise ArgumentError, fn -> ReleaseChecks.release_notes_from_files!("v0.4.0", root) end
  end

  test "checks local page and image links without fetching external URLs", %{directory: root} do
    File.write!(Path.join(root, "target.html"), "present")
    File.write!(Path.join(root, "sample image.png"), "image")
    File.write!(Path.join(root, "a&b.png"), "image")
    page = Path.join(root, "index.html")

    File.write!(page, """
    <a href="target.html#section">ok</a>
    <a href="#local">ok</a>
    <a href="https://example.test/missing">external</a>
    <a href="//example.test/missing">external</a>
    <script src="docs_config.js">const unused = '<img src="ignored.png">';</script>
    <img src="sample%20image.png?version=1" title='href="ignored.png"'>
    <img src='a&amp;b.png'>
    <!-- <a href="ignored.html">comment</a> -->
    &lt;a href="ignored.html"&gt;example&lt;/a&gt;
    <a href="assets-images.html">broken</a>
    <img src=missing.png>
    """)

    assert ReleaseChecks.missing_links(page) == [
             "#{page}: assets-images.html",
             "#{page}: missing.png"
           ]

    assert_raise ArgumentError, ~r/Missing documentation links/, fn ->
      ReleaseChecks.check_doc_links!(root)
    end

    File.write!(Path.join(root, "assets-images.html"), "fixed")
    File.write!(Path.join(root, "missing.png"), "fixed")
    assert ReleaseChecks.check_doc_links!(root) == 3
  end

  test "rejects empty documentation output", %{directory: root} do
    assert_raise ArgumentError, ~r/No HTML documentation/, fn ->
      ReleaseChecks.check_doc_links!(root)
    end
  end

  test "standalone scripts report failures without starting Mix", %{directory: root} do
    executable = System.find_executable("elixir")

    for {script, args, message} <- [
          {"check-doc-links.exs", [root], "No HTML documentation"},
          {"release-notes.exs", [], "usage:"}
        ] do
      {output, status} =
        System.cmd(executable, [Path.join(@root, "scripts/#{script}") | args],
          env: [{"ERL_FLAGS", "+S 2:2"}],
          stderr_to_stdout: true
        )

      assert status == 1
      assert output =~ message
    end
  end

  defp notes(changelog), do: ReleaseChecks.release_notes!("v0.4.0", changelog, "0.4.0", "0.4.0")
end
