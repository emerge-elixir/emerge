Code.require_file("../lib/emerge_skia/build_profiles.ex", __DIR__)
Code.require_file("../mix/native_artifacts.exs", __DIR__)

case System.argv() do
  [builder, version, output] ->
    Emerge.Mix.NativeArtifacts.build!(builder, version, output: output)

  _ ->
    raise ArgumentError, "usage: build-native-artifacts.exs BUILDER VERSION OUTPUT_DIRECTORY"
end
