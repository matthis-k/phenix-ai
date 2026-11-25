"""Check repository-relative inline Markdown links in maintained docs.

Rustdoc links and doctests are validated by Cargo; external URLs and fragments
are not fetched. This deliberately does not enforce document counts or sizes.
"""
import pathlib
import re
import sys
from urllib.parse import unquote, urlsplit

root = pathlib.Path(sys.argv[1]).resolve()
pattern = re.compile(r"\]\((<?[^\s)>]+>?)\)")
errors = []
for path in sorted(root.rglob("*.md")):
    if ".git" in path.parts or "target" in path.parts:
        continue
    fence = None
    for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        marker = re.match(r"^\s{0,3}(`{3,}|~{3,})", line)
        if marker:
            token = marker.group(1)
            if fence is None:
                fence = token
            elif token[0] == fence[0] and len(token) >= len(fence):
                fence = None
            continue
        if fence:
            continue
        for match in pattern.finditer(line):
            dest = urlsplit(match.group(1).strip("<>"))
            if dest.scheme or dest.netloc or not dest.path:
                continue
            target = (path.parent / unquote(dest.path)).resolve()
            if not target.is_relative_to(root) or not target.exists():
                errors.append(f"{path.relative_to(root)}:{number}: missing {dest.path}")
if errors:
    sys.exit("\n".join(errors))
