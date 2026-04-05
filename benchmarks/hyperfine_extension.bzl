load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")

def _hyperfine_extension_impl(module_ctx):
    os_name = module_ctx.os.name
    arch = module_ctx.os.arch

    if "mac" in os_name:
        if arch == "aarch64":
            url = "https://github.com/sharkdp/hyperfine/releases/download/v1.20.0/hyperfine-v1.20.0-aarch64-apple-darwin.tar.gz"
            sha256 = "8ee7067016620447c9d2d6234ec9a4680f958b7ad983549b56334668f63075b5"
            prefix = "hyperfine-v1.20.0-aarch64-apple-darwin"
        else:
            url = "https://github.com/sharkdp/hyperfine/releases/download/v1.20.0/hyperfine-v1.20.0-x86_64-apple-darwin.tar.gz"
            sha256 = "f58d0b90993fadfa122a351428c469ce24afef3865f027f0e6e86f0830d088f1"
            prefix = "hyperfine-v1.20.0-x86_64-apple-darwin"
    elif "linux" in os_name:
        if arch == "aarch64":
            url = "https://github.com/sharkdp/hyperfine/releases/download/v1.20.0/hyperfine-v1.20.0-aarch64-unknown-linux-gnu.tar.gz"
            sha256 = "90875cb1db7a1d797c311174d061728361e58fc70e3b62262a00635ac3b1997c"
            prefix = "hyperfine-v1.20.0-aarch64-unknown-linux-gnu"
        else:
            url = "https://github.com/sharkdp/hyperfine/releases/download/v1.20.0/hyperfine-v1.20.0-x86_64-unknown-linux-gnu.tar.gz"
            sha256 = "63ad53934062118f5b0be11785e0bb1603d4b91667d1921f2fd8df9a8712040a"
            prefix = "hyperfine-v1.20.0-x86_64-unknown-linux-gnu"
    else:
        # Fallback Windows or something else
        url = "https://github.com/sharkdp/hyperfine/releases/download/v1.20.0/hyperfine-v1.20.0-x86_64-pc-windows-msvc.zip"
        sha256 = ""
        prefix = "hyperfine-v1.20.0-x86_64-pc-windows-msvc"

    build_content = """
package(default_visibility = ["//visibility:public"])
exports_files(["hyperfine"])
"""
    if "windows" in os_name:
        build_content = """
package(default_visibility = ["//visibility:public"])
exports_files(["hyperfine.exe"])
"""

    http_archive(
        name = "hyperfine_bin",
        urls = [url],
        sha256 = sha256 if sha256 else None,
        strip_prefix = prefix,
        build_file_content = build_content,
    )

hyperfine_extension = module_extension(
    implementation = _hyperfine_extension_impl,
)
