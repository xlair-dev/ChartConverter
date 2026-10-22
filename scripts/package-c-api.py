import hashlib
import json
import os
import shlex
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path


LIBRARIES = {
    "x86_64-unknown-linux-gnu": "libchart_converter_ffi.a",
    "x86_64-apple-darwin": "libchart_converter_ffi.a",
    "x86_64-pc-windows-msvc": "chart_converter_ffi.lib",
}


def cmake_string(value):
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"').replace(";", "\\;") + '"'


def write_cmake_config(path, library_name, native_libraries):
    arguments = shlex.split(native_libraries)
    link_libraries = []
    link_options = []
    index = 0
    while index < len(arguments):
        argument = arguments[index]
        if argument == "-framework" and index + 1 < len(arguments):
            link_options.append(f"SHELL:-framework {arguments[index + 1]}")
            index += 2
        elif argument.startswith("-l") and len(argument) > 2:
            link_libraries.append(argument[2:])
            index += 1
        elif argument.startswith("-"):
            link_options.append(argument)
            index += 1
        else:
            link_libraries.append(argument)
            index += 1

    lines = [
        "include_guard(GLOBAL)",
        'get_filename_component(_chart_converter_root "${CMAKE_CURRENT_LIST_DIR}/../../.." ABSOLUTE)',
        "if(NOT TARGET ChartConverter::FFI)",
        "  add_library(ChartConverter::FFI STATIC IMPORTED GLOBAL)",
        "  set_target_properties(ChartConverter::FFI PROPERTIES",
        f'    IMPORTED_LOCATION "${{_chart_converter_root}}/lib/{library_name}"',
        '    INTERFACE_INCLUDE_DIRECTORIES "${_chart_converter_root}/include"',
        "  )",
    ]
    for property_name, items in [
        ("INTERFACE_LINK_LIBRARIES", link_libraries),
        ("INTERFACE_LINK_OPTIONS", link_options),
    ]:
        if items:
            lines.append(
                f"  set_property(TARGET ChartConverter::FFI APPEND PROPERTY {property_name}"
            )
            lines.extend(f"    {cmake_string(item)}" for item in items)
            lines.append("  )")
    lines.extend(["endif()", ""])
    path.parent.mkdir(parents=True)
    path.write_text("\n".join(lines), encoding="utf-8")


def run(command, *, env=None, print_output=True):
    result = subprocess.run(
        command,
        check=False,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    if print_output or result.returncode != 0:
        print(result.stdout, end="", flush=True)
    result.check_returncode()
    return result.stdout


def main():
    if len(sys.argv) != 2 or sys.argv[1] not in LIBRARIES:
        raise SystemExit(f"usage: {sys.argv[0]} <target-triple>")

    target = sys.argv[1]
    metadata = json.loads(
        run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1"],
            print_output=False,
        )
    )
    package = next(item for item in metadata["packages"] if item["name"] == "chart-converter-ffi")
    target_dir = Path(metadata["target_directory"])
    version = package["version"]

    with tempfile.TemporaryDirectory() as temporary_directory:
        staging = Path(temporary_directory)
        include_dir = staging / "include"
        include_dir.mkdir()
        environment = os.environ.copy()
        environment["CHART_CONVERTER_HEADER_DIR"] = str(include_dir.resolve())

        output = run(
            [
                "cargo",
                "rustc",
                "--color",
                "never",
                "-p",
                "chart-converter-ffi",
                "--target",
                target,
                "--release",
                "--locked",
                "--",
                "--print",
                "native-static-libs",
            ],
            env=environment,
        )
        native_libraries = [
            line.split("native-static-libs:", 1)[1].strip()
            for line in output.splitlines()
            if "native-static-libs:" in line
        ]
        if not native_libraries:
            raise RuntimeError("rustc did not report the static library's native dependencies")

        library = target_dir / target / "release" / LIBRARIES[target]
        header = include_dir / "chart_converter.h"
        if not library.is_file() or not header.is_file():
            raise FileNotFoundError("the C API library or generated header was not produced")

        package_root = staging / "package"
        (package_root / "include").mkdir(parents=True)
        (package_root / "lib").mkdir()
        cmake_dir = package_root / "lib" / "cmake" / "ChartConverterFFI"
        shutil.copy2(header, package_root / "include" / header.name)
        shutil.copy2(library, package_root / "lib" / library.name)
        shutil.copy2(
            Path("crates/chart-converter-ffi/README.md"), package_root / "README.md"
        )
        (package_root / "native-static-libs.txt").write_text(
            f"Version: {version}\nTarget: {target}\n{native_libraries[0]}\n",
            encoding="utf-8",
        )
        write_cmake_config(
            cmake_dir / "ChartConverterFFIConfig.cmake",
            library.name,
            native_libraries[0],
        )

        smoke_build = staging / "smoke-build"
        run([
            "cmake", "-S", "crates/chart-converter-ffi/tests", "-B", str(smoke_build),
            f"-DCMAKE_PREFIX_PATH={package_root}", "-DCMAKE_BUILD_TYPE=Release",
        ])
        run(["cmake", "--build", str(smoke_build), "--config", "Release"])
        run(["ctest", "--test-dir", str(smoke_build), "-C", "Release", "--output-on-failure"])

        output_dir = Path("dist")
        output_dir.mkdir(exist_ok=True)
        archive = output_dir / f"chart-converter-ffi-{target}.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package_zip:
            for path in sorted(package_root.rglob("*")):
                if path.is_file():
                    name = path.relative_to(package_root).as_posix()
                    entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                    entry.compress_type = zipfile.ZIP_DEFLATED
                    entry.external_attr = 0o100644 << 16
                    package_zip.writestr(entry, path.read_bytes())

        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (output_dir / f"{archive.name}.sha256").write_text(
            f"{digest}  {archive.name}\n",
            encoding="ascii",
        )
        print(f"Created {archive}")


if __name__ == "__main__":
    main()
