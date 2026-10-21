Code.require_file("../mix/native_uploads.exs", __DIR__)

case System.argv() do
  [directory, uploader, node, expected] ->
    Emerge.Mix.NativeUploads.upload!(directory, uploader, node, String.to_integer(expected))

  _ ->
    raise ArgumentError,
          "usage: upload-native-artifacts.exs DIRECTORY UPLOADER_ENTRY NODE EXPECTED_ARCHIVES"
end
