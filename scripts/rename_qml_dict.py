"""One-off: QML property names must start lowercase, so rename the `S` dictionary alias to `dict`."""
import re
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "qt/serika-code/qml"
for path in root.glob("*.qml"):
    text = path.read_text(encoding="utf-8")
    new = text
    new = re.sub(r"property var S:", "property var dict:", new)
    new = re.sub(r"(\b\w+)\.S\b", r"\1.dict", new)
    new = re.sub(r"\(S,", "(dict,", new)
    if new != text:
        path.write_text(new, encoding="utf-8")
        print("updated", path.name)
