import up from "@mcp/up";

function main(): unknown[] {
    return [
        up.echo({label: "check", value: {nested: [1, 2], flag: true}}),
        up.echo({label: "check", value: [1, 2]}),
        up.echo({label: "check", value: null}),
        up.echo({label: "check", value: 42}),
        up.echo({label: "check", value: "hello"})
    ];
}
