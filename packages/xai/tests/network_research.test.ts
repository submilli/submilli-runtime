import { label } from "submilli:test";
import secrets from "submilli:secrets";
import xai from "@submilli/xai";

function main(): void {
    const key = secrets.get("XAI_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: XAI_API_KEY is not bound");
        return;
    }
    label("live X Search returns an attributed research answer");
    const result = xai.research({
        question: "Search X for one recent post from XDevelopers about the X API. Give its URL and a one-sentence description. Use one keyword search only; do not fetch threads or profiles.",
        allowedHandles: ["XDevelopers"],
        maxTurns: 2,
        maxOutputTokens: 4096,
    });
    assert(result.id.length > 0 && result.model.length > 0, "response identity");
    assert(result.status === "completed", "research completed within the test bounds");
    assert(result.text.length > 0 && result.citations.length > 0, "answer with source attribution");
    assert(result.usage.searchCalls !== null && result.usage.searchCalls > 0, "search actually used");
    assert(result.usage.inputTokens !== null && result.usage.inputTokens > 0, "token usage");
    assert(result.usage.costDollars !== null && result.usage.costDollars >= 0, "provider cost");
    label("provider model: " + result.model + "; cost USD: " + JSON.stringify(result.usage.costDollars));

    label("live research reads a specific post URL and available replies");
    const thread = xai.getTweetSummary(result.citations[0].url, "Keep it brief: summarize the post and up to two replies. Use one thread fetch only, without keyword, semantic, or user searches.", true);
    assert(thread.status === "completed" && thread.text.length > 0, "thread research completed");
    assert(thread.usage.searchCalls !== null && thread.usage.searchCalls > 0, "X Search used for the post URL task");
    assert(thread.citations.length > 0, "thread source attribution");
    label("thread cost USD: " + JSON.stringify(thread.usage.costDollars));
}
