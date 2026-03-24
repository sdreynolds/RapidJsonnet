load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")

def _jsonnet_tests_impl(module_ctx):
    http_archive(
        name = "jsonnet_test_suite_source",
        urls = ["https://github.com/google/jsonnet/archive/refs/tags/v0.20.0.tar.gz"],
        sha256 = "77bd269073807731f6b11ff8d7c03e9065aafb8e4d038935deb388325e52511b",
        strip_prefix = "jsonnet-0.20.0",
        build_file = "//third_party_test_suite:jsonnet.BUILD.bazel",
    )

jsonnet_tests = module_extension(
    implementation = _jsonnet_tests_impl,
)
