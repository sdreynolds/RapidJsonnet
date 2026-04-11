load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")

def _jsonnet_tests_impl(module_ctx):
    http_archive(
        name = "jsonnet_test_suite_source",
        urls = ["https://github.com/google/jsonnet/archive/refs/tags/v0.22.0.tar.gz"],
        sha256 = "5914b9904d97efa662d919519cef1a14e4132bfddddaeed8b061b4a8af628f8d",
        strip_prefix = "jsonnet-0.22.0",
        build_file = "//third_party_test_suite:jsonnet.BUILD.bazel",
        patch_cmds = ["rm -f test_suite/BUILD"],
    )

jsonnet_tests = module_extension(
    implementation = _jsonnet_tests_impl,
)
