#!/usr/bin/env nu

const TARGET = 'x86_64-unknown-linux-gnu'
const BINARY = '/usr/local/bin/subroutine-lite-pilot'

def --wrapped checked [command: string, ...args: string] {
    do --capture-errors { ^$command ...$args }
}

def remote [host: string, command: string] {
    checked ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 $host $command
}

def check-schema [host: string] {
    let expected = (
        glob crates/server/migrations/*.sql
        | where {|path| not ($path | str ends-with '.down.sql') }
        | each {|path|
            {
                version: ($path | path basename | split row '_' | first | into int),
                success: 't',
                checksum: (checked shasum -a 384 $path | split row ' ' | first)
            }
        }
        | sort-by version
    )
    let applied = (
        remote $host r#'sudo -n -u postgres psql -X -v ON_ERROR_STOP=1 -At -F ',' -d subroutine_lite_pilot -c "SELECT version, success, encode(checksum, 'hex') FROM _sqlx_migrations ORDER BY version;"'#
        | from csv --noheaders
        | rename version success checksum
        | update version { into int }
    )
    if $applied != $expected {
        error make {msg: 'Migration checksums differ from the Lite pilot database. Deployment stopped without changing the server. Review and explicitly resolve the migration history before retrying; this script never migrates or rewrites checksums.'}
    }
}

# Deploy the Lite pilot only; leave the original simple-server service untouched.
def main [
    --check # Check schema compatibility without building or deploying.
] {
    cd $env.FILE_PWD
    let host = $'felixb@($env.FERMI_IP? | default "fermi")'
    check-schema $host
    if $check {
        print 'Lite pilot migration checksums match.'
        return
    }

    # Zig's parallel linker can exhaust macOS's default descriptor limit.
    if $nu.os-info.name == 'macos' {
        ulimit --file-descriptor-count 8192
    }
    $env.CARGO_TARGET_DIR = ($env.CARGO_TARGET_DIR? | default 'target' | path expand)
    checked cargo zigbuild --locked --release -p server --bin subroutine-lite-server --target $TARGET
    let binary = ($env.CARGO_TARGET_DIR | path join $TARGET release subroutine-lite-server)
    let upload = $'/tmp/subroutine-lite-server-(random uuid)'
    let next = $'($BINARY).next-(random uuid)'
    let backup = $'($BINARY).previous-(date now | format date "%Y%m%dT%H%M%S")-(random uuid)'
    let checksum = (open --raw $binary | hash sha256)
    checked scp -o BatchMode=yes -o ConnectTimeout=10 $binary $'($host):($upload)'

    let deploy = ([
        'set -eu'
        $'upload=($upload)'
        $'next=($next)'
        $'binary=($BINARY)'
        $'backup=($backup)'
        $'checksum=($checksum)'
        r#'
trap 'rm -f "$upload"; sudo -n rm -f "$next"' EXIT
printf '%s  %s\n' "$checksum" "$upload" | sha256sum -c -
sudo -n install -m 755 "$upload" "$next"
sudo -n restorecon "$next"
sudo -n cp -p "$binary" "$backup"
sudo -n mv "$next" "$binary"
if sudo -n systemctl restart subroutine-lite-pilot.service; then
    for delay in 1 1 2 2 4 5; do
        sleep "$delay"
        if sudo -n systemctl is-active --quiet subroutine-lite-pilot.service &&
            [ "$(curl -sS --max-time 5 -o /dev/null -w '%{http_code}' http://127.0.0.1:3300/v1/account)" = 401 ]; then
            echo "Lite pilot deployed successfully. Previous binary: $backup"
            exit 0
        fi
    done
fi
echo 'New server failed its health check; restoring the previous binary.' >&2
sudo -n mv "$backup" "$binary"
sudo -n restorecon "$binary"
sudo -n systemctl restart subroutine-lite-pilot.service
exit 1
'#
    ] | str join (char newline))
    remote $host $deploy
}
