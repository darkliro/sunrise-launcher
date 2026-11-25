import io
import os
import subprocess
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

from tools.ci import structure_check as check

BASE_FILES = {
    "src/main.rs": "fn main() {}\n",
    "src/net.rs": (
        "pub fn get() -> Result<Vec<String>, String> { Ok(read().unwrap()) }\n"
        "pub fn parse() -> Result<u8, std::io::Error> { todo!() }\n"
    ),
    "tools/ci/manual-tests.txt": "# comment\nsunrise-launcher net::tests::live_get by-hand manual # network\n",
}


class Repository:
    def __init__(self, root: Path):
        self.root = root
        self.git("init", "-q", "-b", "main")
        self.git("config", "user.name", "Test")
        self.git("config", "user.email", "test@example.com")

    def git(self, *args: str) -> str:
        return subprocess.run(["git", *args], cwd=self.root, check=True, capture_output=True, text=True).stdout.strip()

    def commit(self, files: dict[str, str | None]) -> str:
        for path, text in files.items():
            target = self.root / path
            if text is None:
                target.unlink()
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(text)
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", "change")
        return self.git("rev-parse", "HEAD")


class StructureCheck(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.repo = Repository(Path(directory.name))
        self.base = self.repo.commit(BASE_FILES)
        self.addCleanup(os.chdir, Path.cwd())
        os.chdir(self.repo.root)

    def run_check(self, head: str) -> tuple[int, str]:
        out = io.StringIO()
        with redirect_stdout(out), redirect_stderr(io.StringIO()):
            code = check.main(["--base", self.base, "--head", head])
        return code, out.getvalue()

    def test_the_base_is_counted(self):
        counts = check.measure(self.base)
        self.assertEqual(counts["src_files"], 2)
        self.assertEqual(counts["unwraps"], 1)
        self.assertEqual(counts["string_errors"], 1)
        self.assertEqual(counts["stale_manual_tests"], 1)
        self.assertEqual((counts["ui_cyclic_modules"], counts["ui_mutual_imports"]), (0, 0))

    def test_a_count_going_up_fails(self):
        head = self.repo.commit({
            "src/glass_extra.rs": "#[allow(dead_code, clippy::too_many_arguments)]\nfn f() { x.unwrap(); }\n",
            "src/lib.rs": '#[path = "glass_extra.rs"]\nmod extra;\n' + "\n" * check.LONG_FILE,
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 1)
        counts = check.measure(head)
        self.assertEqual(counts["src_files"], 4)
        self.assertEqual(counts["long_files"], 1)
        self.assertEqual(counts["path_attributes"], 1)
        self.assertEqual(counts["allow_dead_code"], 1)
        self.assertEqual(counts["allow_clippy"], 1)
        self.assertEqual(counts["unwraps"], 2)
        self.assertEqual(counts["retired_names"], 1)
        self.assertIn("went up", out)

    def test_a_count_going_down_passes(self):
        head = self.repo.commit({
            "src/net.rs": "pub fn get() -> Result<Vec<String>, Error> { read() }\n#[test]\nfn live_get() {}\n",
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 0)
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"], counts["stale_manual_tests"]), (0, 0, 0))
        self.assertNotIn("went up", out)

    def test_test_code_is_not_counted(self):
        test_code = "fn t() -> Result<(), String> { x.unwrap(); Ok(()) }\n"
        head = self.repo.commit({
            "src/net.rs": BASE_FILES["src/net.rs"] + "#[cfg(test)]\nmod tests {\n" + test_code + "}\n",
            "src/net/tests.rs": test_code,
            "src/mods/net_tests.rs": test_code,
            "src/mods/test_support.rs": test_code,
            "src/mods/test_support/fixtures.rs": test_code,
            "tests/play.rs": test_code,
            "crates/smod/tests/index.rs": test_code,
        })
        code, out = self.run_check(head)
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (1, 1))
        self.assertEqual(code, 0, out)

    def test_files_a_test_module_declares_are_test_files(self):
        test_code = "fn t() -> Result<(), String> { x.unwrap(); Ok(()) }\n"
        head = self.repo.commit({
            "src/main.rs": "#[cfg(test)]\nmod harness;\nfn main() {}\n",
            "src/harness/mod.rs": "mod fixtures;\n" + test_code,
            "src/harness/fixtures.rs": test_code,
            "src/store.rs": '#[cfg(test)]\n#[path = "store_checks.rs"]\nmod checks;\n#[cfg(test)]\nmod regressions;\n',
            "src/store_checks.rs": test_code,
            "src/store/regressions.rs": test_code,
        })
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (1, 1))

    def test_a_test_module_declaration_hides_none_of_the_code_after_it(self):
        head = self.repo.commit({
            "src/net.rs": "#[cfg(test)]\nmod regressions;\n" + BASE_FILES["src/net.rs"],
            "src/net/regressions.rs": "fn t() { x.unwrap(); }\n",
        })
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (1, 1))

    def test_test_items_are_left_out_one_by_one(self):
        head = self.repo.commit({
            "src/net.rs": (
                "fn a() -> Result<(), String> { b().unwrap(); Ok(()) }\n"
                "#[cfg(test)]\n#[test]\nfn t() -> Result<(), String> { x.unwrap(); Ok(()) }\n"
                "fn c() {\n    #[cfg(test)]\n    let y = z.with(|v| v.unwrap());\n    d().unwrap(); }\n"
                "#[cfg(test)]\nmod tests {\n    fn u() { x.unwrap(); }\n}\n"
                "fn after_the_tests() { e().unwrap(); }\n"
            ),
        })
        counts = check.measure(head)
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (2, 1))

    def test_only_functions_over_the_line_budget_are_counted(self):
        def item(name: str, body_lines: int) -> str:
            return f"fn {name}() {{\n" + "    let x = 1;\n" * body_lines + "}\n"

        head = self.repo.commit({
            # Start line, 78 body lines and the closing brace: 80 lines exactly.
            "src/net.rs": item("at_the_limit", 78) + item("over_the_limit", 79),
        })
        counts = check.measure(head)
        self.assertEqual(counts["long_functions"], 1)

    def test_test_functions_are_left_out_of_the_length_count(self):
        body = "fn t() {\n" + "    let x = 1;\n" * 100 + "}\n"
        head = self.repo.commit({
            "src/net.rs": BASE_FILES["src/net.rs"] + "#[cfg(test)]\nmod tests {\n" + body + "}\n",
            "src/net/tests.rs": body,
        })
        self.assertEqual(check.measure(head)["long_functions"], 0)

    def test_module_cycles_are_counted_from_crate_paths(self):
        head = self.repo.commit({
            "src/lib.rs": "mod a;\nmod b;\nmod c;\nmod d;\n",
            "src/a.rs": "use crate::b::thing;\nuse crate::{c, d};\n",
            "src/b.rs": "use crate::a::Thing;\n",
            "src/c.rs": "fn local() {}\n",
            "src/d.rs": "fn local() {}\n",
        })
        counts = check.measure(head)
        self.assertEqual((counts["cyclic_modules"], counts["mutual_imports"]), (2, 1))

    def test_ui_module_cycles_are_counted_separately_from_lib(self):
        head = self.repo.commit({
            "src/lib.rs": "mod library;\n",
            "src/library.rs": "use crate::library::Item;\n",
            "src/main.rs": "mod page_a;\nmod page_b;\nfn main() {}\n",
            "src/page_a.rs": "use crate::page_b::Widget;\n",
            "src/page_b.rs": "use crate::page_a::Widget;\n",
        })
        counts = check.measure(head)
        self.assertEqual((counts["cyclic_modules"], counts["mutual_imports"]), (0, 0))
        self.assertEqual((counts["ui_cyclic_modules"], counts["ui_mutual_imports"]), (2, 1))

    def test_a_ui_cycle_going_up_fails_the_ratchet(self):
        head = self.repo.commit({
            "src/main.rs": "mod page_a;\nmod page_b;\nfn main() {}\n",
            "src/page_a.rs": "use crate::page_b::Widget;\n",
            "src/page_b.rs": "use crate::page_a::Widget;\n",
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 1)
        self.assertIn("main.rs mutual module import pairs", out)
        self.assertIn("went up", out)

    def test_gpui_under_lib_fails_the_ratchet_and_comments_do_not_count(self):
        head = self.repo.commit({
            "src/lib.rs": "mod core_mod;\n",
            "src/core_mod.rs": "// gpui is used by the window, not here.\nuse gpui::App;\n",
            "src/main.rs": BASE_FILES["src/main.rs"] + "mod page;\n",
            "src/page.rs": "use gpui::Render;\n// gpui::Window in a comment.\n",
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 1)
        counts = check.measure(head)
        self.assertEqual((counts["gpui_files"], counts["lib_gpui_files"]), (2, 1))
        self.assertIn("files using gpui that lib.rs reaches", out)

    def test_a_ui_split_adding_gpui_files_is_reported_not_ratcheted(self):
        head = self.repo.commit({
            "src/main.rs": "mod settings;\nfn main() {}\n",
            "src/settings/page_a.rs": "use gpui::Render;\n",
            "src/settings/page_b.rs": "use gpui::App;\n",
        })
        code, out = self.run_check(head)
        self.assertEqual(code, 0)
        counts = check.measure(head)
        self.assertEqual((counts["gpui_files"], counts["lib_gpui_files"]), (2, 0))
        self.assertIn("reported, not a ratchet", out)

    def test_a_test_only_fn_in_the_middle_hides_only_itself(self):
        handler = "fn handler() -> Result<(), String> { Ok(()) }\n"
        head = self.repo.commit({
            "src/cli.rs": (
                handler
                + "#[cfg(test)]\nfn attached(cmd: Command) -> Result<i32, String> {\n    run(cmd).unwrap()\n}\n"
                + handler * 3
            ),
        })
        counts = check.measure(head)
        # src/net.rs from the base adds one of each.
        self.assertEqual((counts["unwraps"], counts["string_errors"]), (1, 5))


if __name__ == "__main__":
    unittest.main()
