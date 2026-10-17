# Shared parser JNI

This standalone Cargo workspace depends only on `../../crates/parser`,
`../../crates/core`, and their Rust dependencies. It has its own `Cargo.lock` and
build directory; no Desktop dependency or root workspace membership is required.

## Android build

Prerequisites:

- Rust with `aarch64-linux-android`, `x86_64-linux-android`,
  `armv7-linux-androideabi`, and `i686-linux-android` installed via rustup.
- `cargo-ndk` 4.1.2 and Android SDK NDK `29.0.14206865`.
- Nushell on `PATH` for the helper script (not required for direct Cargo builds).

From the repository root:

```sh
nu android/native/build.nu --api-level 26
```

The SDK argument may be omitted when `ANDROID_HOME` or `ANDROID_SDK_ROOT` is set.
The script sets both NDK path variables for its child process, so an older
inherited `ANDROID_NDK_ROOT` cannot select another NDK. Its default profile is
`release`; `--profile dev` is also supported. Optional positional ABI arguments
restrict the build, for example `arm64-v8a x86_64`.

The four default outputs are:

```text
android/native/target/jniLibs/release/arm64-v8a/libsubroutine_parser_jni.so
android/native/target/jniLibs/release/x86_64/libsubroutine_parser_jni.so
android/native/target/jniLibs/release/armeabi-v7a/libsubroutine_parser_jni.so
android/native/target/jniLibs/release/x86/libsubroutine_parser_jni.so
```

`android/app/build.gradle.kts` registers these outputs as JNI libraries and makes
`preBuild` depend on `buildParserJni`. It tracks this workspace's sources,
manifest, lockfile, build script, shared parser/core sources, and the root
`Cargo.toml` as inputs. The application minSdk and native API level are both 26.
Gradle unit tests depend on `buildParserHost` and load the real host JNI library.
`--offline` also sets `CARGO_NET_OFFLINE=true` for native build tasks.

Only `libsubroutine_parser_jni.so` is packaged. The core crate also emits a cdylib
as a side effect of its existing crate types, but JNI statically includes core
and does not need `libsubroutine_core.so`. The JNI library needs only Android's
`libc.so` and `libdl.so`, not `libc++_shared.so`. NDK 29 produces 16-KB-aligned LOAD
segments for the two 64-bit ABIs; APK ZIP alignment remains the parent's AGP
packaging responsibility.

Without Nushell, from `android/native`, the equivalent compile command is:

```sh
ANDROID_NDK_HOME=/Users/felixb/Library/Android/sdk/ndk/29.0.14206865 ANDROID_NDK_ROOT=/Users/felixb/Library/Android/sdk/ndk/29.0.14206865 cargo ndk -t arm64-v8a -t x86_64 -t armeabi-v7a -t x86 --platform 26 build --release --locked
```

This direct command leaves the library in `target/<Rust target>/release/`;
`build.nu` additionally copies only the JNI library to the generated ABI folders.

## Kotlin boundary

Load `System.loadLibrary("subroutine_parser_jni")` and declare the native method
on **`com.example.subroutine_simple.ui.creator.NativeItemParser`**:

```kotlin
external fun nativeParse(requestJson: String): String
```

The exported symbol is
`Java_com_example_subroutine_1simple_ui_creator_NativeItemParser_nativeParse`.
Keep this class and native method name unchanged under R8; do not put the native
method on a companion class unless it has the corresponding static bridge.
`NativeItemParser.kt` implements the Kotlin boundary; `app/proguard-rules.pro`
keeps the native symbol's declaring class and method name.

Request (JSON v1):

```json
{
  "version": 1,
  "text": "Review every Monday at 9am for 30m",
  "kind": "Action",
  "now": "2026-10-15T12:00:00Z",
  "timezone": "America/New_York",
  "defaultDate": null
}
```

Accepted kinds are exactly `Action`, `Event`, `Marker`, `Signal`, `RoutineStep`,
`ActionTemplate`, and `EventTemplate`. Use `Action` for a routine's title and
`RoutineStep` for its steps. `now` must be UTC RFC3339 (zero offset), `timezone`
an IANA ID, and `defaultDate` either null/absent or strict `YYYY-MM-DD`.

Every normal response has these nine keys:

| Key | JSON type |
| --- | --- |
| `version` | `1` |
| `title` | string |
| `whenValue` | null, `YYYY-MM-DD`, or UTC RFC3339 with `Z` |
| `naiveTime` | null or `HH:MM:SS` |
| `durationSeconds` | null or signed 64-bit integer |
| `recurrence` | null or the serialized core `Recurrence` |
| `warnings` | array of strings |
| `highlights` | array of `{ "start": Int, "end": Int, "kind": String }` |
| `error` | null or string |

Recurrence uses the core schema, not a new Android representation: `rule`,
`end_date`, `remaining`, and an explicit `timezone`. For example, daily is
`{"type":"relative","value":{"unit":"days","interval":1}}`, and Monday is
`{"type":"week","value":["monday"]}`. Multiple recurrence bounds keep the last
bound and emit the shared parser's warning.

Highlights are sorted, half-open UTF-16 ranges into the **original request
text**, not the extracted title. `kind` uses the shared names `Title`, `When`,
`Duration`, and `Recurrence` (the shared enum also reserves other categories).
Conversion checks UTF-8 character boundaries and correctly counts supplementary
characters as two UTF-16 code units. There is no trimming before parsing.

Whitespace-only text returns an empty title, no clauses/highlights/warnings,
and `error: null`. Without recurrence or an explicit `When` highlight,
`whenValue` is null even if a default date was supplied: the caller preserves
manual contextual timing. Bare times use `defaultDate`; explicit dates and
relative phrases use the request's `now` in its requested timezone. Months and
years use checked calendar arithmetic with month-end clamping. DST gaps fail;
folds choose the earlier instant. Action templates return a naive clock time;
event templates retain clock text in their title, matching shared parser behavior.

The adapter limits text to 16,384 UTF-8 bytes and the JSON request to 131,072
UTF-8 bytes. JNI checks the Java string length before allocating a bounded
UTF-16 copy and rejects unpaired surrogates. Invalid input returns a populated
`error` with empty title/clauses; it never echoes the request or private title.
Internal JNI/serialization failures or caught Rust panics raise
`IllegalStateException` unless the VM already has an exception pending. Unwinding
is enabled and contained at the JNI boundary. No global TZ/environment changes,
panic-hook installation, or request logging occur.

## Validation

Run from `android/native` (avoids resolving/building the Desktop workspace):

```sh
cargo test --locked -p parser -p subroutine_parser_jni --lib --tests
cargo clippy --locked -p parser -p subroutine_parser_jni --all-targets -- -D warnings
cargo fmt --package subroutine_parser_jni --check
nu test-jvm.nu --java-home '/Applications/Android Studio.app/Contents/jbr/Contents/Home'
```

The JVM helper builds the real host cdylib and runs a Java class with the exact
JNI name under `-Xcheck:jni`; it is not a mock or Kotlin-only parser test. A
JDK 17+ can be selected with `JAVA_HOME` instead of the flag. The JVM test is
ignored in ordinary Cargo runs because it requires a host library and JDK.
Android cross-compilation proves linking and ABI artifacts, not execution on ART.

Do not use `cargo fmt --all`: Cargo formatting follows path dependencies into
the parent workspace. Format only the named JNI package or explicit parser files.
