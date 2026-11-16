import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
JAVA = os.environ.get("YAPI_TEST_JAVA", "java")
JAVAC = str(Path(JAVA).with_name("javac")) if "/" in JAVA else "javac"

class DependencyReaderTest(unittest.TestCase):
    def test_reads_generic_fields_getter_and_record_without_initializing_classes(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / "Reply.java"
            source.write_text('package fixture;\npublic class Reply<T> {\n private String code; private T data;\n public String getCode() { return code; }\n public T getData() { return data; }\n public boolean isSuccess() { return true; }\n static { try { java.nio.file.Files.writeString(java.nio.file.Path.of(System.getProperty("probe.marker")), "executed"); } catch (Exception e) { throw new RuntimeException(e); } }\n}')
            record = temp / "Ack.java"
            record.write_text("package fixture; public record Ack(String id, boolean accepted) {}")
            subprocess.run([JAVAC, "--release", "17", "-d", str(temp), str(source), str(record)], check=True, capture_output=True)
            marker = temp / "initialized"
            p = subprocess.run([JAVA, "-Dprobe.marker=" + str(marker), "-jar", str(ROOT / "resources/java-types.jar")],
                input=str(temp)+"\nfixture.Reply\nfixture.Ack\nfixture.Missing\n", text=True, capture_output=True)
            self.assertEqual(p.returncode, 0, p.stderr)
            result = json.loads(p.stdout)
            self.assertEqual(sorted(result), ["fixture.Ack", "fixture.Reply"])
            self.assertIn("T data", result["fixture.Reply"])
            self.assertIn("boolean isSuccess()", result["fixture.Reply"])
            self.assertIn("java.lang.String id", result["fixture.Ack"])
            self.assertFalse(marker.exists(), "dependency reader initialized a business class")

    def test_preserves_unsupported_serializer_annotations_for_rejection(self):
        with tempfile.TemporaryDirectory() as work:
            work = Path(work)
            annotation = work / "JsonValue.java"
            annotation.write_text("package com.fasterxml.jackson.annotation; import java.lang.annotation.*; @Retention(RetentionPolicy.RUNTIME) public @interface JsonValue {}")
            source = work / "Special.java"
            source.write_text("package fixture; class Special { String name; @com.fasterxml.jackson.annotation.JsonValue public String encode() { return name; } }")
            subprocess.run([JAVAC, "--release", "17", "-d", str(work), str(annotation), str(source)], check=True, capture_output=True)
            p = subprocess.run([JAVA, "-jar", str(ROOT / "resources/java-types.jar")], input=str(work)+"\nfixture.Special\n", text=True, capture_output=True)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertIn("@JsonValue", json.loads(p.stdout)["fixture.Special"])


    def test_preserves_databind_annotations_for_parser_rejection(self):
        with tempfile.TemporaryDirectory() as work:
            work = Path(work)
            annotation = work / "JsonSerialize.java"
            annotation.write_text("package com.fasterxml.jackson.databind.annotation; import java.lang.annotation.*; @Retention(RetentionPolicy.RUNTIME) public @interface JsonSerialize { Class<?> using(); }")
            source = work / "Special.java"
            source.write_text("package fixture; @com.fasterxml.jackson.databind.annotation.JsonSerialize(using=String.class) class Special { String name; }")
            subprocess.run([JAVAC, "--release", "17", "-d", str(work), str(annotation), str(source)], check=True, capture_output=True)
            p = subprocess.run([JAVA, "-jar", str(ROOT / "resources/java-types.jar")], input=str(work)+"\nfixture.Special\n", text=True, capture_output=True)
            self.assertEqual(p.returncode, 0, p.stderr)
            self.assertIn("@JsonSerialize", json.loads(p.stdout)["fixture.Special"])

if __name__ == "__main__":
    unittest.main()
