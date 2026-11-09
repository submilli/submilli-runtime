# MCP Registry and Glama publication

`server.json` describes a local Docker installation. It does not deploy a
public MCP service. It publishes the container port on loopback and connects to
the `default` Blueprint at `http://127.0.0.1:8128/mcp/default`.

## Prepare the release

Do this only after a human-approved runtime release has published an image with
the `io.modelcontextprotocol.server.name=io.github.submilli/submilli-runtime`
OCI label. Images built before the corresponding Dockerfile change lack this
label. Verify the next release before publishing. Do not retag an earlier image.
For that release, update `server.json`'s root `version`, package `version`, and
the OCI image tag in package `identifier` together.

Resolve the empty `maintainers` list in `glama.json` with verified GitHub
organization Owner usernames before the Glama claim. Do not add a guessed or
contributor username. **TODO: verify organization Owner handles and fill
`maintainers`.**

Check the manifest before publishing:

```sh
mcp-publisher validate server.json
```

The Registry's [GitHub interactive login](https://github.com/modelcontextprotocol/registry/blob/main/docs/modelcontextprotocol-io/authentication.mdx)
cannot currently publish an organization namespace. An organization Owner uses a
GitHub token with either classic `read:org` or fine-grained Organization Members
read-only access. No repository scope is required. Read the token without putting
it in shell history, then run:

```sh
read -rs MCP_REGISTRY_GITHUB_TOKEN
printf '\n'
mcp-publisher login github --token "$MCP_REGISTRY_GITHUB_TOKEN"
unset MCP_REGISTRY_GITHUB_TOKEN
mcp-publisher publish
```

These are manual, human-only steps. Do not add an automated publishing workflow.
The package [ownership requirements](https://github.com/modelcontextprotocol/registry/blob/main/docs/modelcontextprotocol-io/package-types.mdx)
and the [2025-12-11 server schema](https://static.modelcontextprotocol.io/schemas/2025-12-11/server.schema.json)
govern the manifest.

## Provision the local endpoint

The Docker package requires a server token and publishes loopback port 8128.
It does not create a Blueprint. Before connecting a client, run the container,
then apply a reviewed Blueprint whose `name` is `default`.

For a persistent installation, use the published `compose.yaml`. Its
`submilli-state` volume preserves registered Blueprints, sessions, secrets, and
Packages. The package launch includes `--rm`, so a fresh container without that
state requires the Blueprint to be applied again.

With a local Submilli CLI configured for the server, apply the Blueprint after
the server is healthy:

```sh
export SUBMILLI_SERVER_URL=http://127.0.0.1:8128
export SUBMILLI_SERVER_TOKEN='the server admin token'
submilli blueprint lint default.yaml
submilli server blueprint apply default.yaml
```

`default.yaml` must use `name: default`. Define its Packages, Capabilities,
secrets, and filesystem or network access for the intended application before
applying it. The client then connects with Streamable HTTP to
`http://127.0.0.1:8128/mcp/default` and sends the same token as
`Authorization: Bearer <token>`. The Registry schema presents the container
environment variable and HTTP header as separate prompts, so enter the same
token in both.

## Submit the Glama listing

After `glama.json` has verified maintainers, open [Glama's MCP server directory](https://glama.ai/mcp/servers),
click **Add MCP Server**, and submit `https://github.com/submilli/submilli-runtime`.
Then use Glama's **Claim ownership** action. See [Glama's FAQ](https://glama.ai/mcp/faq)
for the current submission flow, and reclaim after listing changes as described
in [Glama's listing guide](https://glama.ai/blog/2025-07-08-what-is-glamajson).
