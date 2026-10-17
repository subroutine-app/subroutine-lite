#!/usr/bin/env nu

# Builds only the isolated parser workspace; Gradle owns packaging.
def main [
    --sdk: path
    --api-level: int = 26
    --profile: string = "release"
    ...abis: string
] {
    let sdk = $sdk | default ($env.ANDROID_HOME? | default $env.ANDROID_SDK_ROOT?)
    if $sdk == null { error make {msg: "Set ANDROID_HOME or pass --sdk <Android SDK>"} }
    let ndk = $sdk | path join ndk '29.0.14206865'
    if not ($ndk | path exists) { error make {msg: $"NDK 29.0.14206865 is not installed at ($ndk)"} }
    if $api_level < 21 { error make {msg: "Android API level must be at least 21"} }
    if $profile not-in [dev release] { error make {msg: "Profile must be dev or release"} }
    let abis = if ($abis | is-empty) { [arm64-v8a x86_64 armeabi-v7a x86] } else { $abis }
    for abi in $abis {
        if $abi not-in [arm64-v8a x86_64 armeabi-v7a x86] {
            error make {msg: $"Unsupported Android ABI: ($abi)"}
        }
    }
    let targets = $abis | each {|abi| [--target $abi] } | flatten
    let target_dir = $env.FILE_PWD | path join target
    let output = $target_dir | path join jniLibs $profile
    cd $env.FILE_PWD
    with-env {ANDROID_NDK_HOME: $ndk, ANDROID_NDK_ROOT: $ndk} {
        ^cargo ndk ...$targets --platform $api_level build --locked --profile $profile --target-dir $target_dir
        if $env.LAST_EXIT_CODE != 0 { error make {msg: "Native parser build failed"} }
    }
    let triples = {
        arm64-v8a: aarch64-linux-android
        x86_64: x86_64-linux-android
        armeabi-v7a: armv7-linux-androideabi
        x86: i686-linux-android
    }
    let profile_dir = if $profile == dev { "debug" } else { "release" }
    $abis | each {|abi|
        let destination = $output | path join $abi
        mkdir $destination
        let library = $target_dir | path join ($triples | get $abi) $profile_dir libsubroutine_parser_jni.so
        cp $library $destination
        $destination | path join libsubroutine_parser_jni.so
    }
}
