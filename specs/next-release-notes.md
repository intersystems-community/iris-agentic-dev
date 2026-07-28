> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes`. The constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.8...<tag>` compare link.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Written against `v0.9.9` as the next tag.

## Notable fixes

### Production management and skill tools broken on Atelier REST connections

`iris_production`, `iris_production_item`, and all skill tools (`skill_list`,
`skill_describe`, `skill_search`, `skill_forget`) returned errors or empty results on any
connection without `IRIS_CONTAINER` set — VS Code extension, remote servers, anything not
running in a named local container.

The same docker-exec fallback bug from v0.9.8 was present across the production lifecycle
functions (status, start, stop, update, recover), the skill execution helper, and the
debug source-map tools (`debug_map_int`, `debug_source_map`). All now use Atelier REST.

## Breaking changes

None.

**Full changelog:**
[`v0.9.8...v0.9.9`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.8...v0.9.9)
