#!/usr/bin/env nu

const TARGET = 'x86_64-unknown-linux-gnu'
const MEMBERS = ['crates/auth' 'crates/core' 'crates/server']
const INPUTS = ['Cargo.toml' 'Cargo.lock' 'LICENSE' 'crates/auth' 'crates/core' 'crates/server']

def --wrapped checked [command: string, ...args: string] {
    do --capture-errors { ^$command ...$args }
}

def remote [host: string, command: string] {
    checked ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 $host $command
}

def migrations [source: path] {
    glob ($source | path join 'crates/server/migrations/*.sql')
    | sort
    | each {|path|
        let name = ($path | path basename)
        {($name): {
            version: ($name | split row '_' | first | into int),
            sha256: (open --raw $path | hash sha256),
            checksum_sha384: (checked shasum -a 384 $path | split row ' ' | first)
        }}
    }
    | reduce --fold {} {|entry, result| $result | merge $entry }
}

def check-schema [host: string, source: path] {
    let expected = (
        migrations $source
        | transpose name migration
        | where {|entry| not ($entry.name | str ends-with '.down.sql') }
        | each {|entry| {
            version: $entry.migration.version,
            success: 't',
            checksum: $entry.migration.checksum_sha384
        }}
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

def source-files [source: path] {
    glob ($source | path join '**/*')
    | where {|path| ($path | path type) == 'file' }
    | sort
    | each {|path| {($path | path relative-to $source): (open --raw $path | hash sha256)} }
    | reduce --fold {} {|entry, result| $result | merge $entry }
}

def locked-packages [path: path] {
    open --raw $path | from toml | get package
    | each {|package| {
        name: $package.name,
        version: $package.version,
        source: ($package.source? | default ''),
        checksum: ($package.checksum? | default '')
    }}
}

def snapshot-working-tree [source: path] {
    let files = (
        checked git --no-pager ls-files --cached --others --exclude-standard -z -- ...$INPUTS
        | split row (char nul)
        | where {|file| ($file | is-not-empty) and ($file | path exists) }
        | uniq
    )
    for file in $files {
        let destination = ($source | path join $file)
        mkdir ($destination | path dirname)
        cp $file $destination
    }
}

# Build and deploy the Lite API working tree to Fermi.
def main [
    --offline # Use cached Cargo dependencies only.
    --check # Check the source snapshot's schema without building or deploying.
    --build-only # Build and validate the bundle without uploading it; still checks Fermi's schema.
    --revision: string # Deploy this committed revision instead of the current working tree.
] {
    cd $env.FILE_PWD
    if $offline {
        $env.CARGO_NET_OFFLINE = 'true'
    }
    let host = $'felixb@($env.FERMI_IP? | default "fermi")'
    let commit = if $revision != null {
        checked git --no-pager rev-parse --verify $'($revision)^{commit}' | str trim
    }
    let source_kind = if $commit == null { 'working-tree' } else { 'commit' }

    let bundle = ($env.FILE_PWD | path join target fermi-deploy $'(date now | format date "%Y%m%dT%H%M%S")-(random uuid)')
    let source = ($bundle | path join source)
    mkdir $source
    if $commit == null {
        snapshot-working-tree $source
    } else {
        let git_archive = ($bundle | path join committed-source.tar)
        checked git archive --format=tar $'--output=($git_archive)' $commit ...$INPUTS
        checked tar -xf $git_archive -C $source
        rm $git_archive
    }
    check-schema $host $source
    if $check {
        print $'Lite pilot migration checksums match the ($source_kind) snapshot.'
        return
    }

    let original_manifest = (open --raw ($source | path join Cargo.toml) | hash sha256)
    let original_lock = (open --raw ($source | path join Cargo.lock) | hash sha256)
    let packages = (locked-packages ($source | path join Cargo.lock))
    let manifest = (open ($source | path join Cargo.toml) | update workspace.members $MEMBERS)
    $manifest | to toml | save --force ($source | path join Cargo.toml)

    # Build the snapshot, not a working tree that can change while Cargo runs.
    $env.CARGO_TARGET_DIR = ($env.CARGO_TARGET_DIR? | default ($env.FILE_PWD | path join target fermi-build) | path expand)
    if $nu.os-info.name == 'macos' { ulimit --file-descriptor-count 8192 }
    cd $source
    checked cargo update --workspace --offline
    let changed_packages = (locked-packages Cargo.lock | where {|package| $package not-in $packages })
    if ($changed_packages | is-not-empty) {
        error make {msg: 'Isolating the server changed dependency versions or checksums. No build was deployed.'}
    }
    let files = (source-files $source)
    checked cargo test --locked -p server -p core -p subroutine-auth --features subroutine-auth/server
    checked cargo zigbuild --locked --release -p server --bin subroutine-lite-server --target $TARGET
    if (source-files $source) != $files {
        error make {msg: 'Build inputs changed while Cargo ran. No build was deployed.'}
    }
    let binary = ($bundle | path join subroutine-lite-server)
    cp ($env.CARGO_TARGET_DIR | path join $TARGET release subroutine-lite-server) $binary
    let archive = ($bundle | path join source.tar)
    with-env {COPYFILE_DISABLE: '1'} {
        checked tar --format=ustar -cf $archive -C $source .
    }
    let migration_files = (migrations $source)
    let initial = ($migration_files | get '0001_init.sql')
    let receipt = {
        built_at: (date now | date to-timezone UTC | format date '%+'),
        artifact_sha256: (open --raw $binary | hash sha256),
        source_archive_sha256: (open --raw $archive | hash sha256),
        migration_sha256: $initial.sha256,
        migration_checksum_sha384: $initial.checksum_sha384,
        migrations: $migration_files,
        build_command: $'cargo zigbuild --locked --release -p server --bin subroutine-lite-server --target ($TARGET)',
        validation_command: 'cargo test --locked -p server -p core -p subroutine-auth --features subroutine-auth/server',
        rustc: (checked rustc --version | str trim),
        zig: (checked zig version | str trim),
        source_kind: $source_kind,
        git_commit: $commit,
        scope: 'Lite API only; source snapshot, isolated workspace, original registry versions retained; no migrations or identity changes',
        original_workspace_sha256: $original_manifest,
        original_lockfile_sha256: $original_lock,
        workspace_members: $MEMBERS,
        source_files: $files
    }
    let receipt_path = ($bundle | path join receipt.json)
    $receipt | to json | save $receipt_path
    let receipt_hash = (open --raw $receipt_path | hash sha256)
    if $build_only {
        print ({bundle: $bundle, source_kind: $source_kind, git_commit: $commit, artifact_sha256: $receipt.artifact_sha256, receipt_sha256: $receipt_hash} | to json)
        return
    }

    let upload = (remote $host 'mktemp -d /tmp/subroutine-lite-deploy.XXXXXXXX' | str trim)
    if $upload !~ '^/tmp/subroutine-lite-deploy\.[A-Za-z0-9]+$' {
        error make {msg: 'Unexpected remote staging path; deployment stopped.'}
    }
    checked scp -o BatchMode=yes -o ConnectTimeout=10 $binary $archive $receipt_path ($env.FILE_PWD | path join script deploy-fermi.py) $'($host):($upload)/'
    remote $host $'sudo -n python3 -B ($upload)/deploy-fermi.py ($upload) ($receipt_hash)'
    remote $host $'rm -f -- ($upload)/subroutine-lite-server ($upload)/source.tar ($upload)/receipt.json ($upload)/deploy-fermi.py; rmdir -- ($upload)'
    print $'Validated source bundle retained at ($bundle)'
}
