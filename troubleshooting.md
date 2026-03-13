# Troubleshooting

## `nexus` command not found

- ensure `~/.local/bin` is on `PATH`
- rerun `./scripts/install.sh --binary ./target/release/nexus`
- or use `./target/release/nexus` directly

## Stats are empty

- confirm `NEXUS_DATABASE_PATH`
- run `nexus init`
- store a test memory and rerun `nexus stats`

## Hooks are missing

- run `nexus hooks status`
- verify the target tool exists in `PATH`
- reinstall with `nexus hooks install --agent <tool>`

## HTTP server does not respond

- start it with `nexus serve --transport http --port 8768`
- check for port conflicts
- verify the process can read the configured database path
