"""One-off: turn qml/Format.js into a QML singleton so every file can use `Format.*` without imports."""
from pathlib import Path

root = Path(__file__).resolve().parent.parent / "qt/serika-code"
js = (root / "qml/Format.js").read_text(encoding="utf-8")
body = "\n".join(line for line in js.splitlines() if not line.strip().startswith(".pragma"))
indented = "\n".join(("    " + line) if line.strip() else "" for line in body.splitlines())
qml = "pragma Singleton\nimport QtQuick\n\nQtObject {\n" + indented + "\n}\n"
(root / "qml/Format.qml").write_text(qml, encoding="utf-8")
(root / "qml/Format.js").unlink()

cmake = (root / "CMakeLists.txt").read_text(encoding="utf-8")
cmake = cmake.replace("qml/Format.js", "qml/Format.qml")
cmake = cmake.replace(
    "set_source_files_properties(qml/Theme.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)",
    "set_source_files_properties(qml/Theme.qml qml/Format.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)",
)
(root / "CMakeLists.txt").write_text(cmake, encoding="utf-8")
print("ok")
