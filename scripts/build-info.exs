# Runs before dependency/native compilation, including from a failed Nerves build.
Code.require_file("../lib/emerge_skia/build_profiles.ex", __DIR__)
Code.require_file("../lib/emerge_skia/build_config/schema.ex", __DIR__)
Code.require_file("../lib/emerge_skia/build_info.ex", __DIR__)
EmergeSkia.BuildInfo.run(System.argv())
