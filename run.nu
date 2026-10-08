def --wrapped main [...args: string] {
    cd $env.FILE_PWD
    ^cargo run -r -p desktop -- ...$args
    exit $env.LAST_EXIT_CODE
}
