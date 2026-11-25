"""The refactor's ratchet (docs/refactoring.md §8): counts that may only go down between a base and a head revision.

Both revisions are read as git objects, so neither needs a checkout. There is no stored baseline: the base commit is
the baseline, and a pull request that lowers a count needs nothing extra.
"""
import argparse
import posixpath
import re
import subprocess
import sys

RETIRED_PREFIXES = ("glass_", "blocks_", "alt_", "launcher_")
LONG_FILE = 1000
LONG_FUNCTION = 80

PATH_ATTRIBUTE = re.compile(r"#\[\s*path\s*=")
ALLOW = re.compile(r"#!?\[\s*(?:cfg_attr\s*\(.*?,\s*)?allow\s*\(([^()]*)\)")
CFG_TEST = re.compile(r"^[ \t]*#\[\s*cfg\s*\(\s*test\s*\)\s*\]", re.M)
# `#[cfg(test)]`, any further attributes, then `mod name;` or `mod name {`, at the start of a line.
TEST_MODULE = re.compile(
    r"^[ \t]*#\[\s*cfg\s*\(\s*test\s*\)\s*\]((?:\s*#\[[^\]]*\])*)\s*(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+(\w+)\s*([;{])",
    re.M,
)
# Any `mod name;` declaration with its attributes, at the start of a line.
MODULE_FILE = re.compile(r"^[ \t]*((?:#\[[^\]]*\]\s*)*)(?:pub(?:\s*\([^)]*\))?\s+)?mod\s+(\w+)\s*;", re.M)
PATH_VALUE = re.compile(r'#\[\s*path\s*=\s*"([^"]+)"\s*\]')
FN = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)")
# A function item at the start of a line, past its visibility and qualifiers.
FUNCTION = re.compile(
    r"^[ \t]*(?:pub(?:\s*\([^)]*\))?\s+)?"
    r"(?:(?:const|async|unsafe|default)\s+)*"
    r"(?:extern\s+\"[^\"]*\"\s+)?"
    r"fn\s+[A-Za-z_][A-Za-z0-9_]*",
    re.M,
)
CRATE_PATH = re.compile(r"\bcrate\s*::\s*([A-Za-z_][A-Za-z0-9_]*)")
CRATE_GROUP = re.compile(r"\bcrate\s*::\s*\{([^{}]*)\}")
CFG_TEST_ATTR = re.compile(r"cfg\s*\(\s*test\s*\)")
GPUI = re.compile(r"\b(?:use\s+gpui\b|gpui\s*::)")

LABELS = {
    "src_files": "files directly under src/",
    "long_files": f".rs files over {LONG_FILE} lines",
    "long_functions": f"non-test fns over {LONG_FUNCTION} lines",
    "cyclic_modules": "lib.rs modules in an import cycle",
    "mutual_imports": "lib.rs mutual module import pairs",
    "ui_cyclic_modules": "main.rs modules in an import cycle",
    "ui_mutual_imports": "main.rs mutual module import pairs",
    "gpui_files": "non-test files using gpui (reported, not a ratchet)",
    "lib_gpui_files": "files using gpui that lib.rs reaches",
    "path_attributes": "#[path attributes",
    "allow_dead_code": "#[allow(dead_code)]",
    "allow_clippy": "#[allow(clippy::…)]",
    "unwraps": ".unwrap() in non-test code",
    "string_errors": "Result<_, String> in non-test code",
    "retired_names": "paths with retired prefixes",
    "stale_manual_tests": "manual-tests.txt entries naming no fn",
}

# Counts the table reports but the check does not enforce. A verbatim split of a UI file adds files
# that mention gpui, so this one follows the number of UI files, not the architecture; the boundary
# that matters is `lib_gpui_files`, which stays at zero.
REPORTED = frozenset({"gpui_files"})


def git(*args: str, data: bytes | None = None) -> bytes:
    return subprocess.run(["git", *args], input=data, check=True, capture_output=True).stdout


def tree(rev: str) -> list[tuple[str, str]]:
    """(blob id, path) for every file at `rev`."""
    entries = []
    for line in git("ls-tree", "-r", "-z", "--full-tree", rev).split(b"\0"):
        if not line:
            continue
        meta, path = line.split(b"\t", 1)
        _mode, kind, blob = meta.split()
        if kind == b"blob":
            entries.append((blob.decode(), path.decode("utf-8", "surrogateescape")))
    return entries


def blobs(ids: list[str]) -> list[str]:
    """The contents of many blobs in one `git cat-file --batch` call."""
    if not ids:
        return []
    out = git("cat-file", "--batch", data="".join(f"{blob}\n" for blob in ids).encode())
    texts, at = [], 0
    for _ in ids:
        header_end = out.index(b"\n", at)
        size = int(out[at:header_end].split()[2])
        start = header_end + 1
        texts.append(out[start:start + size].decode("utf-8", "replace"))
        at = start + size + 1
    return texts


def is_test_file(path: str) -> bool:
    """A test file by its name or folder."""
    parts = path.split("/")
    name = parts[-1]
    return (
        "tests" in parts[:-1]
        or name == "tests.rs"
        or name.endswith("_tests.rs")
        or any(part in ("test_support", "test_support.rs") for part in parts)
    )


def module_paths(declaring: str, name: str, attributes: str) -> list[str]:
    """Where `mod name;` in `declaring` may live: its `#[path]`, else `name.rs` or `name/mod.rs` beside it."""
    directory = posixpath.dirname(declaring)
    given = PATH_VALUE.search(attributes)
    if given:
        return [posixpath.normpath(posixpath.join(directory, given.group(1)))]
    stem = posixpath.basename(declaring).removesuffix(".rs")
    folder = directory if stem in ("lib", "main", "mod") else posixpath.join(directory, stem)
    return [posixpath.join(folder, f"{name}.rs"), posixpath.join(folder, name, "mod.rs")]


def test_files(texts: dict[str, str]) -> set[str]:
    """Test files by name, files a `#[cfg(test)] mod name;` pulls in, and every module those declare in turn."""
    found = {path for path in texts if is_test_file(path)}
    waiting = []
    for path, text in texts.items():
        for attributes, name, opens in TEST_MODULE.findall(text):
            if opens == ";":
                waiting += module_paths(path, name, attributes)
    while waiting:
        path = waiting.pop()
        if path in texts and path not in found:
            found.add(path)
            waiting += [child for attributes, name in MODULE_FILE.findall(texts[path])
                        for child in module_paths(path, name, attributes)]
    return found


def item_end(text: str, at: int) -> int:
    """Where the item starting at `at` ends: after its first `;` or its outermost `{ … }`."""
    depth = 0
    while at < len(text):
        char = text[at]
        if char in "([{":
            depth += 1
        elif char in ")]}":
            depth -= 1
            if char == "}" and depth == 0:
                return at + 1
        elif char == ";" and depth == 0:
            return at + 1
        at += 1
    return at


def non_test_code(text: str) -> str:
    """A non-test file's non-test code: none of its `#[cfg(test)]` items, and nothing from its first inline
    `#[cfg(test)] mod name { … }` on. A `#[cfg(test)] mod name;` declaration's file is a test file ([`test_files`]).
    """
    kept, at = [], 0
    for found in CFG_TEST.finditer(text):
        if found.start() < at:
            continue
        kept.append(text[at:found.start()])
        module = TEST_MODULE.match(text, found.start())
        if module and module.group(3) == "{":
            return "".join(kept)
        at = item_end(text, found.end())
    kept.append(text[at:])
    return "".join(kept)


def long_functions(code: str) -> int:
    """Function items whose body spans more than `LONG_FUNCTION` lines."""
    count = 0
    for found in FUNCTION.finditer(code):
        brace = code.find("{", found.end())
        semicolon = code.find(";", found.end())
        if brace < 0 or 0 <= semicolon < brace:
            continue
        # One line per newline plus the line the item starts on.
        if code.count("\n", found.start(), item_end(code, brace)) >= LONG_FUNCTION:
            count += 1
    return count


def declared_modules(text: str) -> list[tuple[str, str]]:
    """`(attributes, name)` for each `mod name;` declaration that is not a test module."""
    return [(attributes, name) for attributes, name in MODULE_FILE.findall(text)
            if not CFG_TEST_ATTR.search(attributes)]


def module_tree(root: str, texts: dict[str, str]) -> dict[str, set[str]]:
    """The files of each top-level module declared in `root`, following its `mod` declarations."""
    modules = {}
    for attributes, name in declared_modules(texts[root]):
        files, pending = set(), module_paths(root, name, attributes)
        while pending:
            path = pending.pop()
            if path in files or path not in texts:
                continue
            files.add(path)
            pending += [child for attributes, child in declared_modules(texts[path])
                        for child in module_paths(path, child, attributes)]
        modules[name] = files
    return modules


def module_edges(files: set[str], nodes: set[str], texts: dict[str, str]) -> set[str]:
    """The top-level modules that `files` name through `crate::`."""
    found = set()
    for path in files:
        code = non_test_code(texts[path])
        found.update(name for name in CRATE_PATH.findall(code) if name in nodes)
        for group in CRATE_GROUP.findall(code):
            found.update(item.strip().split("::")[0] for item in group.split(",")
                         if item.strip().split("::")[0] in nodes)
    return found


def cycles(modules: dict[str, set[str]], texts: dict[str, str]) -> tuple[int, int]:
    """Modules in a cycle, and mutual module pairs (`a` names `b` and `b` names `a`)."""
    nodes = set(modules)
    edges = {name: module_edges(files, nodes, texts) for name, files in modules.items()}
    index, low, stack, on_stack = {}, {}, [], set()
    cyclic, at = 0, 0

    def visit(node: str) -> None:
        nonlocal at, cyclic
        index[node] = low[node] = at
        at += 1
        stack.append(node)
        on_stack.add(node)
        for child in edges[node]:
            if child not in index:
                visit(child)
                low[node] = min(low[node], low[child])
            elif child in on_stack:
                low[node] = min(low[node], index[child])
        if low[node] == index[node]:
            component = []
            while True:
                child = stack.pop()
                on_stack.discard(child)
                component.append(child)
                if child == node:
                    break
            if len(component) > 1:
                cyclic += len(component)

    for node in nodes:
        if node not in index:
            visit(node)
    mutual = sum(1 for name in edges for child in edges[name] if name in edges[child]) // 2
    return cyclic, mutual


def string_errors(text: str) -> int:
    """`Result<T, String>` with any T, nested generics included."""
    count = 0
    for found in re.finditer(r"\bResult\s*<", text):
        depth, comma, at = 1, None, found.end()
        while at < len(text) and depth:
            char = text[at]
            if char in "<([":
                depth += 1
            elif char in ">)]" and text[at - 1] != "-":
                depth -= 1
            elif char == "," and depth == 1 and comma is None:
                comma = at
            elif char in ";{}":
                break
            at += 1
        if depth == 0 and comma is not None and text[comma + 1:at - 1].strip() == "String":
            count += 1
    return count


def has_retired_name(path: str) -> bool:
    return any(part.startswith(RETIRED_PREFIXES) for part in path.split("/"))


def manual_test_names(text: str) -> list[str]:
    """The last path segment of each test that tools/ci/manual-tests.txt names."""
    names = []
    for line in text.splitlines():
        fields = line.split("#", 1)[0].split()
        if len(fields) >= 2:
            names.append(fields[1].rsplit("::", 1)[-1])
    return names


def measure(rev: str) -> dict[str, int]:
    files = tree(rev)
    rust = [(blob, path) for blob, path in files if path.endswith(".rs")]
    counts = dict.fromkeys(LABELS, 0)
    counts["src_files"] = sum(1 for _, path in files if path.startswith("src/") and path.count("/") == 1)
    counts["retired_names"] = sum(1 for _, path in files if has_retired_name(path))
    texts = dict(zip((path for _, path in rust), blobs([blob for blob, _ in rust])))
    tests = test_files(texts)
    modules = module_tree("src/lib.rs", texts) if "src/lib.rs" in texts else {}
    ui_modules = module_tree("src/main.rs", texts) if "src/main.rs" in texts else {}
    lib_files = set().union(*modules.values()) if modules else set()
    functions = set()
    for path, text in texts.items():
        counts["long_files"] += len(text.splitlines()) > LONG_FILE
        counts["path_attributes"] += len(PATH_ATTRIBUTE.findall(text))
        for lints in ALLOW.findall(text):
            for lint in (part.strip() for part in lints.split(",")):
                counts["allow_dead_code"] += lint == "dead_code"
                counts["allow_clippy"] += lint.startswith("clippy::")
        code = "" if path in tests else non_test_code(text)
        counts["long_functions"] += long_functions(code)
        counts["unwraps"] += code.count(".unwrap()")
        counts["string_errors"] += string_errors(code)
        if path.startswith("src/") and GPUI.search(code):
            counts["gpui_files"] += 1
            if path in lib_files:
                counts["lib_gpui_files"] += 1
        functions.update(FN.findall(text))
    counts["cyclic_modules"], counts["mutual_imports"] = cycles(modules, texts)
    counts["ui_cyclic_modules"], counts["ui_mutual_imports"] = cycles(ui_modules, texts)
    manual = [blob for blob, path in files if path == "tools/ci/manual-tests.txt"]
    if manual:
        names = manual_test_names(blobs(manual)[0])
        counts["stale_manual_tests"] = sum(1 for name in names if name not in functions)
    return counts


def table(base: dict[str, int], head: dict[str, int]) -> str:
    width = max(len(label) for label in LABELS.values())
    lines = [f"{'':{width}}  {'base':>6}  {'head':>6}  {'change':>6}"]
    for key, label in LABELS.items():
        change = head[key] - base[key]
        mark = "  went up" if change > 0 and key not in REPORTED else ""
        lines.append(f"{label:{width}}  {base[key]:>6}  {head[key]:>6}  {change:>+6}{mark}")
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--base", required=True, help="the revision the change is based on")
    parser.add_argument("--head", default="HEAD", help="the revision to check (default: HEAD)")
    args = parser.parse_args(argv)
    base, head = measure(args.base), measure(args.head)
    print(table(base, head))
    risen = [LABELS[key] for key in LABELS if key not in REPORTED and head[key] > base[key]]
    if risen:
        print(f"\nThese went up, and may only go down (docs/refactoring.md §8): {', '.join(risen)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
