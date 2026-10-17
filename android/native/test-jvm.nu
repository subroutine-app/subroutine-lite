#!/usr/bin/env nu

def main [--java-home: path] {
    let java_home = $java_home | default $env.JAVA_HOME?
    if $java_home == null { error make {msg: "Set JAVA_HOME or pass --java-home <JDK 17+>"} }
    let target_dir = $env.FILE_PWD | path join target
    let library = match $nu.os-info.name {
        macos => "libsubroutine_parser_jni.dylib"
        windows => "subroutine_parser_jni.dll"
        _ => "libsubroutine_parser_jni.so"
    }
    cd $env.FILE_PWD
    ^cargo build --locked --target-dir $target_dir
    if $env.LAST_EXIT_CODE != 0 { error make {msg: "Host JNI build failed"} }
    with-env {
        JAVA_HOME: $java_home
        SUBROUTINE_PARSER_JNI_LIBRARY: ($target_dir | path join debug $library)
    } {
        ^cargo test --locked --target-dir $target_dir --test jvm -- --ignored
        if $env.LAST_EXIT_CODE != 0 { error make {msg: "JVM JNI contract test failed"} }
    }
}
