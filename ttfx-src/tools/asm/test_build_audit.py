"""Build-script gate tests using simulated NASM outputs, not audited ASM evidence."""

import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class AuditGateTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.compiler_dir = tempfile.TemporaryDirectory()
        cls.build_script = Path(cls.compiler_dir.name) / "build-script"
        subprocess.run(["rustc", "--edition=2021", str(ROOT / "build.rs"), "-o", str(cls.build_script)], check=True)

    @classmethod
    def tearDownClass(cls):
        cls.compiler_dir.cleanup()

    def run_build(self, audit=None, unchecked=None):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "asm").mkdir()
            (root / "asm/tests.asm").write_text("EXPORT ttfx_test_example\n")
            nasm = root / "nasm"
            nasm.write_text("#!/usr/bin/env python3\nimport pathlib, sys\npathlib.Path(sys.argv[sys.argv.index('-o') + 1]).write_bytes(b'simulated object')\n")
            nasm.chmod(0o755)
            if audit is not None:
                (root / "tools/asm").mkdir(parents=True)
                (root / "tools/asm/isa-audit.sh").write_text("#!/bin/bash\n" + audit + "\n")
            env = os.environ.copy()
            env.update(CARGO_CFG_TARGET_ARCH="x86_64", CARGO_CFG_TARGET_OS="linux", CARGO_FEATURE_ASM="1", OUT_DIR=str(root / "out"), NASM=str(nasm))
            env.pop("TTFX_ASM_UNCHECKED_TIERS", None)
            if unchecked is not None:
                env["TTFX_ASM_UNCHECKED_TIERS"] = unchecked
            (root / "out").mkdir()
            result = subprocess.run([str(self.build_script)], cwd=root, env=env, capture_output=True, text=True)
            return result

    def test_missing_auditor_does_not_link_asm_by_default(self):
        result = self.run_build()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertNotIn("cargo::rustc-link-lib=static=ttfx_asm", result.stdout)
        self.assertIn("could not audit asm tier 1", result.stdout)

    def test_auditor_error_does_not_link_asm_by_default(self):
        result = self.run_build(audit="echo 'audit tool failed' >&2; exit 2")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("audit tool failed", result.stdout)

    def test_unchecked_opt_in_explicitly_links_simulated_tiers(self):
        result = self.run_build(unchecked="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("cargo::rustc-cfg=ttfx_asm_tier=\"4\"", result.stdout)
        self.assertIn("cargo::warning=UNCHECKED", result.stdout)

    def test_opt_in_remains_labeled_unsafe_even_when_audit_reports_success(self):
        result = self.run_build(audit="exit 0", unchecked="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("cargo::warning=UNCHECKED", result.stdout)

    def test_other_unchecked_values_do_not_opt_in(self):
        result = self.run_build(unchecked="true")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)

    def test_simulated_audit_rejection_excludes_tiers(self):
        result = self.run_build(audit="echo 'above tier'; exit 1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("above tier", result.stdout)

    def test_audit_success_links_all_simulated_tiers(self):
        result = self.run_build(audit="exit 0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        for tier in range(1, 5):
            self.assertIn(f'cargo::rustc-cfg=ttfx_asm_tier="{tier}"', result.stdout)

    def test_baseline_rejection_disables_all_asm_by_default(self):
        audit = 'case "$3" in *ttfx_asm_tier.o) echo "baseline rejected"; exit 1;; esac\nexit 0'
        result = self.run_build(audit=audit)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertNotIn("cargo::rustc-link-lib=static=ttfx_asm", result.stdout)
        self.assertIn("baseline rejected", result.stdout)

    def test_baseline_tool_error_disables_all_asm_by_default(self):
        audit = 'case "$3" in *ttfx_asm_tier.o) echo "objdump failed" >&2; exit 2;; esac\nexit 0'
        result = self.run_build(audit=audit)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("objdump failed", result.stdout)

    def test_unchecked_baseline_rejection_warns_explicitly(self):
        audit = 'case "$3" in *ttfx_asm_tier.o) echo "baseline rejected"; exit 1;; esac\nexit 0'
        result = self.run_build(audit=audit, unchecked="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("cargo::rustc-cfg=ttfx_asm\n", result.stdout)
        self.assertIn("linking UNCHECKED (unsafe scratch opt-in)", result.stdout)

    def test_auditor_rejects_empty_disassembly(self):
        self.assert_audit_rejects("fake disassembly\\n")

    def test_auditor_rejects_unparsed_instruction_line(self):
        self.assert_audit_rejects("  0:\\t90 \\tnop\\n  1:\\tunrecognized-format\\n")

    def test_auditor_accepts_parsed_baseline_instruction(self):
        with tempfile.TemporaryDirectory() as directory:
            fake_objdump = Path(directory) / "objdump"
            fake_objdump.write_text("#!/bin/sh\nprintf '  0:\\t90 \\tnop\\n'\n")
            fake_objdump.chmod(0o755)
            env = os.environ.copy()
            env["OBJDUMP"] = str(fake_objdump)
            script = Path(__file__).with_name("isa-audit.sh")
            result = subprocess.run(["bash", str(script), "-q", "1", "/dev/null"], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def assert_audit_rejects(self, output):
        with tempfile.TemporaryDirectory() as directory:
            fake_objdump = Path(directory) / "objdump"
            fake_objdump.write_text("#!/bin/sh\nprintf '%b' '" + output + "'\n")
            fake_objdump.chmod(0o755)
            env = os.environ.copy()
            env["OBJDUMP"] = str(fake_objdump)
            script = Path(__file__).with_name("isa-audit.sh")
            result = subprocess.run(["bash", str(script), "-q", "1", "/dev/null"], env=env, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
