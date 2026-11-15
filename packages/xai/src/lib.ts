import { post } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import { report } from "submilli:cost";
import { ResearchOptions, ResearchResult, buildResearchBody, buildTweetSummaryQuestion, decodeResearchResponse, XaiError, xaiHttpError } from "./internal";

export { ResearchOptions, ResearchResult, Citation, ResearchUsage, XaiError } from "./internal";

/** Research public X posts and replies in one request, using only Grok X Search.
 * @param options Research question and optional search and generation controls. Credentials are supplied internally.
 * @returns Generated answer, citations, completed/incomplete status, and provider usage/cost. Not an exhaustive post or reply listing.
 * @capability x.ai/research { model: string }
 */
export function research(options: ResearchOptions): ResearchResult {
    const model = options.model ?? "grok-4.7";
    const body = buildResearchBody({
        question: options.question, model: model,
        allowedHandles: options.allowedHandles, excludedHandles: options.excludedHandles,
        fromDate: options.fromDate, toDate: options.toDate,
        maxTurns: options.maxTurns, maxOutputTokens: options.maxOutputTokens,
        enableImageUnderstanding: options.enableImageUnderstanding,
        enableVideoUnderstanding: options.enableVideoUnderstanding,
    });
    check("x.ai/research", { model: model });
    const key = secrets.get("XAI_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        throw new XaiError("missing_credentials", "Bind XAI_API_KEY in the blueprint before using X research");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    const response = post("https://api.x.ai/v1/responses", body, headers);
    if (!response.ok) throw xaiHttpError(response.status, response.headers.get("retry-after") ?? null);
    const result = decodeResearchResponse(response.body);
    reportCost(result);
    return result;
}

/** Summarize a specific X/Twitter post using Grok X Search. Requires x.ai/research for grok-4.7.
 * @param url HTTPS post URL on x.com or twitter.com; sharing parameters are removed.
 * @param instruction What to explain or emphasize; defaults to a concise summary.
 * @param includeReplies Include available replies in the summary, default false. This controls the requested answer, not what xAI fetches internally.
 * @returns The same research result as research(), including citations, completion status, and usage.
 */
export function getTweetSummary(url: string, instruction: string = "Summarize the post clearly and concisely.", includeReplies: boolean = false): ResearchResult {
    const question = buildTweetSummaryQuestion(url, instruction, includeReplies);
    return research({ question: question });
}

function reportCost(result: ResearchResult): void {
    const usage: { [unit: string]: number } = {};
    if (result.usage.inputTokens !== null) usage["input_tokens"] = result.usage.inputTokens;
    if (result.usage.outputTokens !== null) usage["output_tokens"] = result.usage.outputTokens;
    if (result.usage.costDollars === null && result.usage.inputTokens === null && result.usage.outputTokens === null) return;
    // Reporting must not discard an already paid answer when the caller denies cost.report.
    try {
        report({
            usd: result.usage.costDollars ?? undefined,
            usage: usage,
            label: result.model,
        });
    } catch (error: unknown) {
        // The provider's usage remains available on the result.
    }
}
