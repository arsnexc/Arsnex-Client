#!/usr/bin/env bash
# Standalone test run for the mod core — no Gradle, no Minecraft, no network.
# Compiles only the classes that do not reference Minecraft types.
set -e
cd "$(dirname "$0")"
OUT=$(mktemp -d)

# Minecraft 1.20.4 requires Java 17; the code uses switch expressions and
# records, so a Java 11 javac cannot compile it. Prefer a local JDK 17 if the
# system default is older.
JAVAC=javac
JAVA=java
for cand in "$JAVA_HOME/bin" /home/user/jdk17/bin /usr/lib/jvm/java-17-openjdk-amd64/bin; do
  if [ -x "$cand/javac" ] && "$cand/javac" -version 2>&1 | grep -qE 'javac 1[7-9]|javac 2[0-9]'; then
    JAVAC="$cand/javac"; JAVA="$cand/java"; break
  fi
done
echo "using $($JAVAC -version 2>&1)"

$JAVAC -encoding UTF-8 -d "$OUT" -sourcepath stub-slf4j:src/main/java \
  src/main/java/dev/arsex/mod/module/*.java \
  src/main/java/dev/arsex/mod/config/*.java \
  src/main/java/dev/arsex/mod/modules/Zoom.java \
  src/main/java/dev/arsex/mod/modules/Cps.java \
  src/main/java/dev/arsex/mod/modules/FpsCounter.java \
  src/main/java/dev/arsex/mod/modules/Coordinates.java
$JAVAC -encoding UTF-8 -cp "$OUT" -d "$OUT" src/test/java/Harness.java
$JAVA -cp "$OUT" Harness
