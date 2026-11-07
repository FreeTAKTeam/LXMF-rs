#!/usr/bin/env python3
"""Exercise the include! fragment ratchet on throwaway source trees."""

from pathlib import Path
import tempfile
import unittest

from check_module_includes import BASELINE, read_baseline, scan, violations, write_baseline


class ModuleIncludesTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        (self.root / "docs/architecture").mkdir(parents=True)
        self.write("crates/libs/a/src/lib.rs", 'include!("lib_parts/one.rs");\nfn a() {}\n')
        self.write("crates/libs/a/src/lib_parts/one.rs", 'include!("two.rs");\n')
        self.write("crates/libs/a/src/lib_parts/two.rs", "fn two() {}\nfn three() {}\n")
        self.write("crates/libs/a/src/data.rs", 'const D: &str = include_str!("x.rs");\n')

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def write(self, name: str, text: str) -> None:
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)

    def test_nested_fragments_count_toward_root(self) -> None:
        self.assertEqual(scan(self.root), {"crates/libs/a/src/lib.rs": (2, 5)})

    def test_new_root_and_grown_count_fail(self) -> None:
        baseline = {"crates/libs/a/src/lib.rs": 1}
        self.write("xtask/src/main.rs", 'include!("main_parts/x.rs");\n')
        self.write("xtask/src/main_parts/x.rs", "\n")
        found = violations(scan(self.root), baseline)
        self.assertEqual(len(found), 2)
        self.assertIn("2 fragments > baseline 1", found[0])
        self.assertIn("xtask/src/main.rs: new include!", found[1])
        with self.assertRaises(ValueError):
            write_baseline(self.root / BASELINE, scan(self.root), baseline)

    def test_stale_entry_fails_and_rewrite_drops_it(self) -> None:
        baseline = {"crates/libs/a/src/lib.rs": 2, "crates/libs/gone.rs": 4}
        roots = scan(self.root)
        self.assertEqual(violations(roots, baseline), ["crates/libs/gone.rs: no longer includes fragments; remove it from the baseline"])
        write_baseline(self.root / BASELINE, roots, baseline)
        self.assertEqual(read_baseline(self.root / BASELINE), {"crates/libs/a/src/lib.rs": 2})

    def test_missing_fragment_is_an_error(self) -> None:
        self.write("crates/apps/b/src/main.rs", 'include!("nope.rs");\n')
        with self.assertRaises(ValueError):
            scan(self.root)


if __name__ == "__main__":
    unittest.main()
