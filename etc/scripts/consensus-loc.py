#!/usr/bin/env python3
"""Count physical Rust lines in consensus service, engine, CLI and RPC.

Run from the repository root, optionally passing a baseline Git revision.
Nonblank, non-comment lines are split into production and tests. Test directories,
test_utils, tests.rs, *_test.rs and inline cfg(test)/test-utils items are tests.
Only src/**/*.rs is measured: documentation files, generated build output and
external fixtures are outside the input. Comments and blank lines are separate.
This is a source-layout counter, not a Rust preprocessor or logical-LoC metric.
"""
import collections
import json
import pathlib
import re
import subprocess
import sys

roots = [f"crates/consensus/{name}/src" for name in ("service", "engine", "cli", "rpc")]
revision = sys.argv[1] if len(sys.argv) > 1 else None
command = (["git", "ls-tree", "-r", "--name-only", revision, "--", *roots]
           if revision else ["git", "ls-files", "--cached", "--others", "--exclude-standard", *roots])
paths = subprocess.check_output(command, text=True).splitlines()
totals = collections.defaultdict(collections.Counter)
for path in sorted(set(paths)):
    if not path.endswith(".rs") or (not revision and not pathlib.Path(path).exists()):
        continue
    text = (subprocess.check_output(["git", "show", f"{revision}:{path}"], text=True)
            if revision else pathlib.Path(path).read_text())
    whole_test = any(part in ("tests", "test_utils", "fixtures") for part in path.split("/")) or bool(
        re.search(r"/(tests|.*_test)\.rs$", path))
    test_item = False
    opened = False
    depth = 0
    block_comment = False
    for line in text.splitlines():
        stripped = line.strip()
        if not test_item and stripped in (
                '#[cfg(test)]', '#[cfg(any(test, feature = "test-utils"))]',
                '#[cfg(all(test, feature = "metrics"))]'):
            test_item, opened, depth = True, False, 0
        category = "test" if whole_test or test_item else "production"
        if not stripped:
            category = "blank"
        elif block_comment or stripped.startswith(("//", "/*")):
            category = "comment"
        totals[path.split("/")[2]][category] += 1
        if stripped.startswith("/*") and "*/" not in stripped:
            block_comment = True
        if block_comment and "*/" in stripped:
            block_comment = False
        if test_item and not stripped.startswith(("//", "#[")):
            code = re.sub(r'"(?:\\.|[^"\\])*"', '""', stripped).split("//")[0]
            opened |= "{" in code
            depth += code.count("{") - code.count("}")
            if (opened and depth == 0) or (not opened and code.endswith(";")):
                test_item = False
print(json.dumps(dict(totals), indent=2, sort_keys=True))
