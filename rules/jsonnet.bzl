"""Bazel rules for building JSON from Jsonnet using RapidJsonnet."""

def _q(s):
    """Minimally shell-quote a path by wrapping in single quotes."""
    return "'" + s.replace("'", "'\\''") + "'"

JsonnetLibraryInfo = provider(
    doc = "Provides transitive Jsonnet sources and data files.",
    fields = {
        "srcs": "depset of this library's source file",
        "transitive_srcs": "depset of all transitive Jsonnet source files",
        "data": "depset of all transitive data files",
    },
)

def _collect_transitive(deps):
    """Collect transitive srcs and data from JsonnetLibraryInfo deps."""
    transitive_srcs = []
    transitive_data = []
    for dep in deps:
        if JsonnetLibraryInfo in dep:
            info = dep[JsonnetLibraryInfo]
            transitive_srcs.append(info.transitive_srcs)
            transitive_data.append(info.data)
    return transitive_srcs, transitive_data

def _jsonnet_library_impl(ctx):
    src_file = ctx.file.src
    transitive_srcs_deps, transitive_data_deps = _collect_transitive(ctx.attr.deps)

    direct_files = [src_file]

    if ctx.attr.precompile_bytecode:
        compiled = ctx.actions.declare_file(src_file.basename + "c")
        ctx.actions.run(
            outputs = [compiled],
            inputs = [src_file],
            executable = ctx.executable._compiler,
            arguments = [src_file.path, compiled.path],
            mnemonic = "JsonnetCompile",
            progress_message = "Precompiling Jsonnet bytecode: %s" % ctx.label,
        )
        direct_files.append(compiled)

    srcs_depset = depset([src_file])
    transitive_srcs_depset = depset(direct_files, transitive = transitive_srcs_deps)
    data_depset = depset(ctx.files.data, transitive = transitive_data_deps)

    all_files = depset(transitive = [transitive_srcs_depset, data_depset])

    return [
        DefaultInfo(files = all_files),
        JsonnetLibraryInfo(
            srcs = srcs_depset,
            transitive_srcs = transitive_srcs_depset,
            data = data_depset,
        ),
    ]

jsonnet_library = rule(
    implementation = _jsonnet_library_impl,
    attrs = {
        "src": attr.label(
            allow_single_file = True,
            mandatory = True,
            doc = "A single Jsonnet source file.",
        ),
        "deps": attr.label_list(
            providers = [JsonnetLibraryInfo],
            doc = "jsonnet_library dependencies.",
        ),
        "data": attr.label_list(
            allow_files = True,
            doc = "Data files available at runtime.",
        ),
        "precompile_bytecode": attr.bool(
            default = False,
            doc = "If True, ahead-of-time compile the source to bytecode using jsonnet_compiler.",
        ),
        "_compiler": attr.label(
            default = Label("//:jsonnet_compiler"),
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Collects a Jsonnet source file and its dependencies for use by jsonnet_to_json.",
)

def _jsonnet_to_json_impl(ctx):
    # Resolve output filename
    out_name = ctx.attr.out if ctx.attr.out else ctx.label.name + ".json"
    output = ctx.actions.declare_file(out_name)

    # Get the main source file
    main_info = ctx.attr.main[JsonnetLibraryInfo]
    main_src = main_info.srcs.to_list()[0]

    # Collect all transitive inputs from main + deps
    transitive_srcs_deps = [main_info.transitive_srcs]
    transitive_data_deps = [main_info.data]

    dep_srcs, dep_data = _collect_transitive(ctx.attr.deps)
    transitive_srcs_deps.extend(dep_srcs)
    transitive_data_deps.extend(dep_data)

    all_srcs = depset(transitive = transitive_srcs_deps)
    all_data = depset(ctx.files.data, transitive = transitive_data_deps)
    all_inputs = depset(transitive = [all_srcs, all_data])

    ctx.actions.run_shell(
        outputs = [output],
        inputs = all_inputs,
        tools = [ctx.executable._tool],
        command = "{tool} -q -J . {src} > {out}".format(
            tool = _q(ctx.executable._tool.path),
            src = _q(main_src.path),
            out = _q(output.path),
        ),
        mnemonic = "Jsonnet",
        progress_message = "Compiling Jsonnet to JSON: %s" % ctx.label,
    )

    return [DefaultInfo(files = depset([output]))]

jsonnet_to_json = rule(
    implementation = _jsonnet_to_json_impl,
    attrs = {
        "main": attr.label(
            providers = [JsonnetLibraryInfo],
            mandatory = True,
            doc = "The jsonnet_library target containing the entrypoint file.",
        ),
        "deps": attr.label_list(
            providers = [JsonnetLibraryInfo],
            doc = "Additional jsonnet_library dependencies.",
        ),
        "data": attr.label_list(
            allow_files = True,
            doc = "Data files available at runtime.",
        ),
        "out": attr.string(
            doc = "Output filename. Defaults to <target_name>.json.",
        ),
        "_tool": attr.label(
            default = Label("//:main"),
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Compiles a Jsonnet file to JSON using the RapidJsonnet binary.",
)
