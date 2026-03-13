# Memory Types

Nexus stores memories under a primary category defined in `nexus-core`.

## Core Categories

- `general`
- `facts`
- `preferences`
- `context`
- `specifications`
- `session`

## Example

```bash
nexus store \
  --content "User prefers concise release notes" \
  --agent codex \
  --category preferences \
  --labels docs,release
```

## Notes

- categories are the primary classification surface
- labels and metadata can provide additional detail
- `specifications` is intended for reusable task and requirement context
