"""构建随插件分发的只读 Java 元数据读取器，需要 JDK 17+。"""
import os
from pathlib import Path
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
LIBS = [ROOT / "vendor/asm" / f"{name}-9.7.1.jar" for name in ["asm", "asm-util"]]
JAVAC = os.environ.get("JAVAC", "javac")
with tempfile.TemporaryDirectory() as work:
    subprocess.run([JAVAC, "--release", "17", "-encoding", "UTF-8", "-cp", os.pathsep.join(map(str, LIBS)),
                    "-d", work, str(ROOT / "java/DependencyTypes.java")], check=True)
    dest = ROOT / "resources/java-types.jar"
    dest.parent.mkdir(exist_ok=True)
    entries = {"META-INF/MANIFEST.MF": b"Manifest-Version: 1.0\nMain-Class: DependencyTypes\n\n"}
    for lib in LIBS:
        with zipfile.ZipFile(lib) as archive:
            for name in archive.namelist():
                if name.endswith(".class") and name != "module-info.class":
                    entries[name] = archive.read(name)
    for path in Path(work).rglob("*.class"):
        entries[path.relative_to(work).as_posix()] = path.read_bytes()
    license_path = ROOT / "vendor/asm/LICENSE"
    if license_path.exists():
        entries["META-INF/ASM-LICENSE"] = license_path.read_bytes()
    with zipfile.ZipFile(dest, "w", compression=zipfile.ZIP_DEFLATED) as archive:
        for name, data in sorted(entries.items()):
            info = zipfile.ZipInfo(name, (2020, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data)
print("Built resources/java-types.jar")
