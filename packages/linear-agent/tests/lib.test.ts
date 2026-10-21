import { label } from "submilli:test";
import {
    Activity, ActivityPage, ActivitySignal, Choice, ExternalLink, Session,
    ThoughtOptions, ActionOptions, AskOptions, RespondOptions, FailOptions,
    AskChoiceOptions, AskAccountConnectionOptions, askAccountConnection,
} from "@submilli/linear-agent";

function main(): void {
    label("history decodes all six content kinds and known signals");
    const contents = [
        '{"type":"thought","body":"Working"}',
        '{"type":"action","action":"Searching","parameter":"issues","result":null}',
        '{"type":"elicitation","body":"Which repository?"}',
        '{"type":"response","body":"Ready"}',
        '{"type":"error","body":"Failed"}',
        '{"type":"prompt","body":"Use backend"}',
    ];
    const signals: ActivitySignal[] = ["auth", "select", "stop", "continue"];
    for (let i = 0; i < contents.length; i++) {
        const content = contents[i];
        const signal = signals[i % signals.length];
        const decoded = JSON.parse('{"id":"activity","createdAt":"2026-01-01T00:00:00Z","ephemeral":false,"content":' + content + ',"signal":"' + signal + '","signalMetadata":null}') as Activity;
        assert(decoded.id === "activity", "identity preserved");
        assert(decoded.signal === signal, "known signal preserved");
    }
    const page = JSON.parse('{"nodes":[],"pageInfo":{"hasNextPage":false,"endCursor":null}}') as ActivityPage;
    assert(page.pageInfo.endCursor === null, "nullable terminal cursor");
    const auth = JSON.parse('{"id":"a","createdAt":"t","ephemeral":false,"content":{"type":"elicitation","body":"Connect"},"signal":"auth","signalMetadata":{"url":"https://example.com","providerName":"GitHub","userId":"u"}}') as Activity;
    assert(auth.signalMetadata?.userId === "u", "account target decoded");
    const select = JSON.parse('{"id":"a","createdAt":"t","ephemeral":false,"content":{"type":"elicitation","body":"Choose"},"signal":"select","signalMetadata":{"options":[{"label":null,"value":"v"}]}}') as Activity;
    assert(select.signalMetadata?.options?.[0].value === "v", "upstream nullable choice labels decoded");

    label("exported options preserve false, empty results, UUIDs and account fields");
    const thought: ThoughtOptions = { id: "uuid", ephemeral: false };
    const action: ActionOptions = { result: "", ephemeral: true };
    const ask: AskOptions = { id: "uuid" };
    const respond: RespondOptions = { id: "uuid" };
    const fail: FailOptions = { id: "uuid" };
    const choiceOptions: AskChoiceOptions = { id: "uuid" };
    const connection: AskAccountConnectionOptions = { providerName: "GitHub", userId: "u" };
    const choice: Choice = { label: "Backend", value: "backend" };
    const link: ExternalLink = { label: "PR", url: "https://example.com/pr" };
    assert(thought.ephemeral === false && action.result === "", "explicit option values preserved");
    assert(ask.id === respond.id && fail.id === choiceOptions.id, "all emitters accept an ID");
    assert(connection.userId === "u" && choice.value === "backend" && link.label === "PR", "exported input types compile");
    const session = JSON.parse('{"id":"s","status":"awaitingInput","url":null,"summary":null,"issue":null,"comment":{"id":"c"},"externalUrls":[],"plan":null}') as Session;
    assert(session.status === "awaitingInput" && session.comment?.id === "c", "session result type compiles");

    label("invalid account URLs fail before reading credentials or making requests");
    for (const url of ["http://example.com", "mailto:user@example.com", "", "/relative", "https://"]) {
        let failed = false;
        try { askAccountConnection("s", "Connect", url); } catch (error) { failed = true; }
        assert(failed, "reject invalid account link");
    }
}
