#!/usr/bin/env bash
# Standalone test run — no Gradle, no network. Verifies the core in ~2s.
set -e
cd "$(dirname "$0")"
OUT=$(mktemp -d)
javac -encoding UTF-8 -d "$OUT" -sourcepath stub-slf4j:src/main/java \
  src/main/java/dev/arsex/module/*.java src/main/java/dev/arsex/ui/*.java
javac -encoding UTF-8 -cp "$OUT" -d "$OUT" src/test/java/Harness.java
java -cp "$OUT" Harness
