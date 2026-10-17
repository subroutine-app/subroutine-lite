#!/usr/bin/env nu

const ISSUER = 'https://auth.subroutineapp.com/auth/v1/'

def probe [url: string] {
    try {
        http get --full --allow-errors --raw --redirect-mode manual --max-time 15sec $url
    } catch {
        {status: 0, body: '', headers: {response: []}}
    }
}

def header [response: record, name: string] {
    $response.headers.response
    | where {|entry| ($entry.name | str lowercase) == $name }
    | get value
    | str join ','
    | str lowercase
}

def cache-is-private [value: string] {
    $value | split row ',' | all {|directive| ($directive | str trim) == 'no-store' }
}


def ssh-read [command: string] {
    try {
        ^ssh -o BatchMode=yes -o ConnectTimeout=10 -o ServerAliveInterval=5 -o ServerAliveCountMax=2 felixb@fermi $command | complete
    } catch {
        {exit_code: 1, stdout: '', stderr: ''}
    }
}

def check [name: string, passed: bool, detail: string] {
    {check: $name, passed: $passed, detail: $detail}
}

def main [--json] {
    let discovery_response = probe $'($ISSUER).well-known/openid-configuration'
    let discovery = try { $discovery_response.body | from json } catch { {} }
    let discovery_ok = (
        $discovery_response.status == 200
        and $discovery.issuer? == $ISSUER
        and $discovery.authorization_endpoint? == $'($ISSUER)oidc/authorize'
        and $discovery.token_endpoint? == $'($ISSUER)oidc/token'
        and $discovery.jwks_uri? == $'($ISSUER)oidc/certs'
        and 'S256' in ($discovery.code_challenge_methods_supported? | default [])
    )
    let jwks_response = probe $'($ISSUER)oidc/certs'
    let jwks = try { $jwks_response.body | from json } catch { {} }
    let signing_key = ($jwks.keys? | default [] | any {|key|
        (
            $key.kty? == 'RSA' and $key.alg? == 'RS256' and $key.use? == 'sig'
            and ($key.kid? | default '' | is-not-empty)
            and ($key.n? | default '' | is-not-empty)
            and ($key.e? | default '' | is-not-empty)
        )
    })
    let account = probe 'https://lite-api.subroutineapp.com/v1/account'
    let surfaces = [
        {name: 'Marketing site', url: 'https://subroutineapp.com/', expected: 200}
        {name: 'Original browser app', url: 'https://app.subroutineapp.com/', expected: 200}
        {name: 'Original API auth boundary', url: 'https://api.subroutineapp.com/v1/account', expected: 401}
    ] | par-each {|surface|
        let response = probe $surface.url
        check $surface.name ($response.status == $surface.expected) $'HTTP ($response.status), expected ($surface.expected); no redirects'
    }
    let units = ssh-read 'timeout 15s systemctl is-active rauthy-lite-pilot.service subroutine-lite-pilot.service nginx.service cloudflared.service postgresql.service subroutine-lite-pilot-backup.timer'
    let states = ($units.stdout | lines | where {|line| $line != '' })
    let backup_command = ssh-read 'sudo -n timeout --kill-after=5s 45s python3 -B /etc/subroutine-lite-pilot/backup/backup.py status'
    let backup = try { $backup_command.stdout | from json } catch { {} }
    let backup_ok = (
        $backup_command.exit_code == 0
        and $backup.repository_metadata_check? == 'ok'
        and $backup.pending_restart? == false
        and $backup.failure_since_success? == false
        and $backup.service?.Result? == 'success'
        and $backup.service?.ExecMainStatus? == '0'
        and ($backup.age_hours? | default 1000000) >= 0
        and ($backup.age_hours? | default 1000000) <= 36
    )
    let checks = [
        (check 'Public OIDC discovery' $discovery_ok 'HTTPS issuer/endpoints and S256; TLS verified')
        (check 'Public signing keys' ($jwks_response.status == 200 and $signing_key) 'Public RS256 key; no credentials')
        (check 'Lite API auth boundary' ($account.status == 401) $'Anonymous HTTP ($account.status), expected 401')
        (check 'Lite API cache policy' (cache-is-private (header $account 'cache-control')) 'Anonymous 401 must not be cached')
        (check 'Lite API HSTS' ((header $account 'strict-transport-security') == 'max-age=31536000') 'HSTS max-age=31536000')
        (check 'Fermi services and backup timer' ($units.exit_code == 0 and ($states | length) == 6 and ($states | all {|state| $state == 'active' })) 'Rauthy, Lite, nginx, tunnel, PostgreSQL, backup timer: all active')
        (check 'Local encrypted backup' $backup_ok 'Snapshot <=36h, restic metadata OK, no recovery/failure; off-host not checked')
    ] | append $surfaces
    let passed = ($checks | all {|item| $item.passed })
    let report = {
        checked_at: (date now | date to-timezone UTC | format date '%+'),
        scope: 'Pilot health now, not launch readiness',
        passed: $passed,
        checks: $checks
    }
    if $json {
        print ($report | to json)
    } else {
        print $report.scope
        print ($checks | table)
    }
    if not $passed { exit 1 }
}
