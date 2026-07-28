> Staging file. When the next tag is cut, this becomes the GitHub release body via
> `gh release edit <tag> --notes`. The constitution requires What's new / Notable fixes /
> Breaking changes plus a `v0.9.7...<tag>` compare link.
> There is no `CHANGELOG.md` in this repo; the release body is the changelog.
>
> Written against `v0.9.8` as the next tag.

## Notable fixes

### BPL and DTL introspection broken on Atelier REST connections

`docs_introspect` and `extract_message_map_routing` returned empty `xdata_flow` or
NOT_FOUND for BPL and DTL classes on any connection without `IRIS_CONTAINER` set. That
covers every Atelier REST-only setup: the VS Code extension, connections to remote
servers, and any instance not running in a named local container.

The export step inside both functions used the docker exec path instead of Atelier REST.
When docker exec was unavailable, the function returned nothing silently — no error, no
indication of why. Both now use Atelier REST for the export. BPL routing, DTL routing,
and `xdata_flow` introspection work on any connection that can reach Atelier REST.

## Breaking changes

None.

**Full changelog:**
[`v0.9.7...v0.9.8`](https://github.com/intersystems-community/iris-agentic-dev/compare/v0.9.7...v0.9.8)
