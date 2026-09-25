// Native runtime coverage: response casts and union serialization are runtime
// checked by Submilli, unlike the Node HTTP-contract harness.
import { label } from "submilli:test";
import { AgentActivity, AgentActivityCreateInput, AgentSession, AgentSessionUpdateInput, Comment } from "@submilli/linear";

function main(): void {
    label("activity unions decode user stop signals and nullable action results");
    const stopped = JSON.parse('{"id":"a","createdAt":"2026-01-01T00:00:00Z","ephemeral":false,"content":{"type":"prompt","body":"Stop"},"signal":"stop","signalMetadata":null}') as AgentActivity;
    assert(stopped.content.type === "prompt", "incoming prompts are readable");
    assert(stopped.signal === "stop", "stop signal is preserved");
    const action = JSON.parse('{"id":"a","createdAt":"2026-01-01T00:00:00Z","ephemeral":true,"content":{"type":"action","action":"Search","parameter":"repo","result":null},"signal":null,"signalMetadata":null}') as AgentActivity;
    assert(action.content.type === "action", "action variant decodes");

    label("all emitted content variants survive JSON round trips");
    const inputs: AgentActivityCreateInput[] = [
        { agentSessionId: "s", content: { type: "thought", body: "Working" }, ephemeral: true },
        { agentSessionId: "s", content: { type: "action", action: "Search", parameter: "repo" } },
        { agentSessionId: "s", content: { type: "response", body: "Done" } },
        { agentSessionId: "s", content: { type: "error", body: "Failed" } },
        { agentSessionId: "s", content: { type: "elicitation", body: "Choose" }, signal: "select", signalMetadata: { options: [{ label: "Runtime", value: "runtime" }] } },
        { agentSessionId: "s", content: { type: "elicitation", body: "Connect" }, signal: "auth", signalMetadata: { url: "https://example.com/auth", providerName: "GitHub" } },
    ];
    for (const input of inputs) {
        const decoded = JSON.parse(JSON.stringify(input)) as AgentActivityCreateInput;
        assert(decoded.content.type === input.content.type, "activity type preserved");
        assert(decoded.agentSessionId === "s", "session ID preserved");
    }
    assert(!JSON.stringify(inputs[1]).includes("result"), "absent action result omitted");
    assert(!JSON.stringify(inputs[2]).includes("signal"), "absent signal omitted");

    label("session JSON decodes nullable fields and a populated plan");
    const session = JSON.parse('{"id":"s","status":"active","url":null,"summary":null,"issue":{"id":"i"},"comment":null,"externalUrls":[{"label":"Console","url":"https://example.com"}],"plan":[{"content":"Inspect","status":"inProgress"}]}') as AgentSession;
    assert(session.plan !== null, "plan decoded");
    if (session.plan !== null) assert(session.plan[0].status === "inProgress", "step status preserved");
    assert(session.externalUrls[0].label === "Console", "external link decoded");
    const update: AgentSessionUpdateInput = { plan: [], summary: null };
    assert(JSON.stringify(update) === '{"plan":[],"summary":null}', "explicit clears sent without changing links");

    label("threaded comments expose the parent ID while old fixtures remain valid");
    const reply = JSON.parse('{"id":"c","body":"Reply","user":null,"parent":{"id":"root"}}') as Comment;
    assert(reply.parent !== null, "thread parent decoded");
    if (reply.parent !== null) assert(reply.parent.id === "root", "parent ID preserved");
    const old = JSON.parse('{"id":"c","body":"Old","user":null}') as Comment;
    assert(old.parent === null, "old comment fixtures need no parent field");
}
