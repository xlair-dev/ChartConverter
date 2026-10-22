import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import unittest
from unittest import mock


PACKAGE = runpy.run_path(str(Path(__file__).with_name("package-c-api.py")))
RUN = PACKAGE["run"]
MAIN = PACKAGE["main"]


class PackageCApiTests(unittest.TestCase):
    def test_success_returns_and_prints_output(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = RUN([sys.executable, "-c", "print('build succeeded')"])
        self.assertEqual(result, "build succeeded\n")
        self.assertEqual(output.getvalue(), result)

    def test_quiet_success_returns_output_without_printing(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            result = RUN([sys.executable, "-c", "print('metadata')"], print_output=False)
        self.assertEqual(result, "metadata\n")
        self.assertEqual(output.getvalue(), "")

    def test_failure_always_prints_combined_output_and_preserves_exit_code(self):
        command = [
            sys.executable,
            "-c",
            "import sys; print('build started', flush=True); "
            "print('compiler error', file=sys.stderr, flush=True); sys.exit(7)",
        ]
        for print_output in [False, True]:
            with self.subTest(print_output=print_output):
                output = io.StringIO()
                with contextlib.redirect_stdout(output):
                    with self.assertRaises(subprocess.CalledProcessError) as caught:
                        RUN(command, print_output=print_output)
                self.assertEqual(caught.exception.returncode, 7)
                self.assertEqual(caught.exception.cmd, command)
                self.assertIn("build started", output.getvalue())
                self.assertIn("compiler error", output.getvalue())
                self.assertEqual(output.getvalue(), caught.exception.output)

    def test_native_library_query_disables_color_even_in_ci(self):
        metadata = json.dumps({
            "packages": [{"name": "chart-converter-ffi", "version": "0.1.0"}],
            "target_directory": "unused-target",
        })
        # Stop after capturing the build command; no Rust build or package is needed.
        run = mock.Mock(side_effect=[metadata, RuntimeError("stop before build")])
        with mock.patch.dict(os.environ, {"CARGO_TERM_COLOR": "always"}):
            with mock.patch.object(sys, "argv", ["package-c-api.py", "x86_64-unknown-linux-gnu"]):
                with mock.patch.dict(MAIN.__globals__, {"run": run}):
                    with self.assertRaisesRegex(RuntimeError, "stop before build"):
                        MAIN()
        command = run.call_args.args[0]
        self.assertEqual(command[:4], ["cargo", "rustc", "--color", "never"])
        self.assertEqual(command[-3:], ["--", "--print", "native-static-libs"])
        self.assertEqual(run.call_args.kwargs["env"]["CARGO_TERM_COLOR"], "always")


if __name__ == "__main__":
    unittest.main()
