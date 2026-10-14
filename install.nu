def --wrapped checked [command: string, ...args: string] {
    ^$command ...$args
    if $env.LAST_EXIT_CODE != 0 {
        error make {msg: $'($command) failed with exit code ($env.LAST_EXIT_CODE).'}
    }
}

def install-linux [feature_args: list<string>] {
    $env.CARGO_TARGET_DIR = (
        $env.CARGO_TARGET_DIR? | default 'target' | path expand
    )
    checked sh script/bundle-linux --release ...$feature_args

    let compiler = (^rustc -vV | complete)
    if $compiler.exit_code != 0 {
        error make {msg: $'Could not determine the Rust host target: ($compiler.stderr)'}
    }
    let target = ($compiler.stdout | lines | parse 'host: {target}' | get 0.target)
    let archive = (
        $env.CARGO_TARGET_DIR
        | path join $target release bundle linux $'subroutine-lite-($target).tar.gz'
    )
    let staging = (mktemp --directory)
    try {
        checked tar -xzf $archive -C $staging
        checked python3 ($staging | path join subroutine-lite install)
    } catch {|err|
        rm --recursive --force $staging
        error make $err
    }
    rm --recursive --force $staging
}

def main [
    --offline
    --features: string = ""
] {
    cd $env.FILE_PWD
    if $offline {
        $env.CARGO_NET_OFFLINE = 'true'
    }
    let feature_args = if ($features | is-empty) { [] } else { ['--features' $features] }
    match $nu.os-info.name {
        'macos' => { checked sh script/bundle-mac --release --install ...$feature_args }
        'linux' => { install-linux $feature_args }
        'windows' => {
            let feature_args = if ($features | is-empty) { [] } else { ['-Features' $features] }
            checked pwsh -NoProfile -ExecutionPolicy Bypass -File script/bundle-windows.ps1 -Install ...$feature_args
        }
        _ => { error make {msg: $'Unsupported platform: ($nu.os-info.name). Expected macOS, Linux, or Windows.'} }
    }
}
