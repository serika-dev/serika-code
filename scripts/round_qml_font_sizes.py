"""One-off: Qt's font.pixelSize is an int, so round the half-pixel sizes up."""
import math
import re
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "qt/serika-code/qml"
for path in root.glob("*.qml"):
    text = path.read_text(encoding="utf-8")
    new = re.sub(
        r"font\.pixelSize:\s*(\d+)\.(\d+)\b",
        lambda m: f"font.pixelSize: {math.floor(float(m.group(1) + '.' + m.group(2)) + 0.5)}",
        text,
    )
    if new != text:
        path.write_text(new, encoding="utf-8")
        print("updated", path.name)
