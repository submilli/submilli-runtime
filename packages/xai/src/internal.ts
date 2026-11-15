/** One research task. Search filters do not restrict parent/reply thread fetches. */
export interface ResearchOptions {
    /** Nonblank task, including any relevant post URLs and evidence requirements. */
    question: string;
    /** Defaults to grok-4.7; must support X Search through the Responses API. */
    model?: string;
    /** Handles without @; at most 20. Mutually exclusive with excludedHandles. */
    allowedHandles?: string[];
    /** Exclude these usernames from searches; at most 20, without @. */
    excludedHandles?: string[];
    /** UTC dates, YYYY-MM-DD. Start inclusive, end exclusive. */
    fromDate?: string;
    /** Exclusive UTC end date, YYYY-MM-DD. */
    toDate?: string;
    /** Package default 2, range 1–10. Bounds rounds, not posts or dollar spend. */
    maxTurns?: number;
    /** Package default 4096, range 128–32768; includes reasoning tokens. */
    maxOutputTokens?: number;
    /** Both default false. Media understanding incurs additional token costs. */
    enableImageUnderstanding?: boolean;
    /** Analyze videos found by X Search; defaults false. */
    enableVideoUnderstanding?: boolean;
}

/** A source attributed by Grok. */
export interface Citation {
    /** Provider-attributed HTTP(S) source URL. */
    url: string;
    /** Provider title or label, null when absent. */
    title: string | null;
}

/** Provider-reported totals for this request; missing counters remain null. */
export interface ResearchUsage {
    /** Total input tokens consumed, when reported. */
    inputTokens: number | null;
    /** Output tokens, including reasoning, when reported. */
    outputTokens: number | null;
    /** Total tokens consumed, when reported. */
    totalTokens: number | null;
    /** Number of provider-side X Search calls. */
    searchCalls: number | null;
    /** Fetched posts, including repeated fetches and thread context. */
    postsFetched: number | null;
    /** User profiles returned by user search. */
    usersFetched: number | null;
    /** Provider total cost in USD, converted from integer ticks. */
    costDollars: number | null;
}

/** One research outcome; always inspect status and refusals. */
export interface ResearchResult {
    /** Provider response ID. */
    id: string;
    /** Model that actually answered. */
    model: string;
    /** incomplete may contain a partial or empty answer. */
    status: "completed" | "incomplete";
    /** Provider's reason for incomplete output, when supplied. */
    incompleteReason: string | null;
    /** Assistant text blocks joined with newlines; may be empty on truncation/refusal. */
    text: string;
    /** Sources in output order, with duplicates preserved. */
    citations: Citation[];
    /** Explicit model refusals, separate from sourced research text. */
    refusals: string[];
    /** Provider usage and cost; missing values stay null. */
    usage: ResearchUsage;
}

/** Local failures use status 0; provider HTTP failures preserve Retry-After. */
export class XaiError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;
    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "XaiError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Validate and snapshot before any credential read or network access.
 * @param options Research task and controls.
 * @returns Serialized Responses API request body.
 */
export function buildResearchBody(options: ResearchOptions): string {
    if (typeof options.question !== "string" || options.question.trim().length === 0) throw invalidArgument("question must not be blank");
    const model = options.model ?? "grok-4.7";
    if (model.trim().length === 0) throw invalidArgument("model must not be blank");
    const turns = options.maxTurns ?? 2;
    const tokens = options.maxOutputTokens ?? 4096;
    integerRange(turns, 1, 10, "maxTurns");
    integerRange(tokens, 128, 32768, "maxOutputTokens");
    if (options.allowedHandles !== undefined && options.excludedHandles !== undefined) throw invalidArgument("allowedHandles and excludedHandles are mutually exclusive");
    const tool = new Map<string, unknown>();
    tool.set("type", "x_search");
    addHandles(tool, "allowed_x_handles", options.allowedHandles);
    addHandles(tool, "excluded_x_handles", options.excludedHandles);
    addDate(tool, "from_date", options.fromDate);
    addDate(tool, "to_date", options.toDate);
    if (options.fromDate !== undefined && options.toDate !== undefined && options.fromDate >= options.toDate) throw invalidArgument("fromDate must be before the exclusive toDate");
    if (options.enableImageUnderstanding !== undefined) tool.set("enable_image_understanding", options.enableImageUnderstanding);
    if (options.enableVideoUnderstanding !== undefined) tool.set("enable_video_understanding", options.enableVideoUnderstanding);
    return JSON.stringify({
        model: model,
        input: [{ role: "user", content: options.question }],
        tools: [tool],
        max_turns: turns,
        max_output_tokens: tokens,
        store: false,
        stream: false,
    });
}

/** Build a grounded post-summary task from a validated URL.
 * @param url HTTPS X/Twitter post URL, including username/status, i/status, or i/web/status forms.
 * @param instruction Nonblank directions for the summary.
 * @param includeReplies Whether the answer should discuss available replies.
 * @returns A question using a canonical post URL and explicit reply-summary instructions.
 */
export function buildTweetSummaryQuestion(url: string, instruction: string, includeReplies: boolean): string {
    const match = /^https:\/\/(?:(?:www|mobile)\.)?(?:x\.com|twitter\.com)\/(?:[A-Za-z0-9_]{1,15}\/status|i\/status|i\/web\/status)\/([1-9][0-9]{0,19})(?:\/(?:photo|video)\/[1-9][0-9]*)?\/?(?:[?#][^\s]*)?$/i.exec(url);
    if (match === null || /\s/.test(url)) throw invalidArgument("url must be an HTTPS X/Twitter post URL with a numeric post ID");
    if (instruction.trim().length === 0) throw invalidArgument("instruction must not be blank");
    const postUrl = "https://x.com/i/status/" + match[1];
    const replies = includeReplies
        ? "Include a summary of available replies, clearly separated from the original post. Cite the replies discussed. If no replies are available, say so; do not invent them."
        : "Summarize only the original post. Do not discuss or summarize replies.";
    return "Read the specific X post at " + postUrl + " using X Search's thread fetch. " +
        "Identify the author and summarize what the post says with a source citation. " +
        "If the post is unavailable, deleted, or inaccessible, state that clearly instead of inventing its content.\n" +
        replies + "\nSummary instructions:\n" + instruction;
}

interface ResponseEnvelope {
    id: string;
    model: string;
    status: string;
    output: unknown[];
    error?: unknown;
    incomplete_details?: { reason?: string | null } | null;
    usage?: ApiUsage | null;
}

interface ApiUsage {
    input_tokens?: number | null;
    output_tokens?: number | null;
    total_tokens?: number | null;
    cost_in_usd_ticks?: number | null;
    server_side_tool_usage_details?: {
        x_search_calls?: number | null;
        x_posts_fetched?: number | null;
        x_users_fetched?: number | null;
    } | null;
}

/** Decode typed results without disguising provider failures or missing usage.
 * @param body Provider response JSON.
 * @returns Completed or incomplete research, including citations and usage.
 */
export function decodeResearchResponse(body: string): ResearchResult {
    try {
        const data = JSON.parse(body) as ResponseEnvelope;
        if (data.error !== undefined && data.error !== null) throw new XaiError("provider_error", "xAI reported a response error; research did not complete");
        if (data.status === "failed" || data.status === "cancelled") throw new XaiError("provider_error", "xAI research " + data.status);
        if (data.id.trim().length === 0 || data.model.trim().length === 0 || !Array.isArray(data.output)) throw invalidResponse();
        if (data.status !== "completed" && data.status !== "incomplete") throw invalidResponse();
        const texts: string[] = [];
        const citations: Citation[] = [];
        const refusals: string[] = [];
        for (const value of data.output) readMessage(value, texts, citations, refusals);
        if (data.status === "completed" && texts.length === 0 && refusals.length === 0) throw invalidResponse();
        return {
            id: data.id, model: data.model, status: data.status,
            incompleteReason: data.incomplete_details?.reason ?? null,
            text: texts.join("\n"), citations: citations, refusals: refusals,
            usage: readUsage(data.usage),
        };
    } catch (cause) {
        if (cause instanceof XaiError) throw cause;
        throw invalidResponse();
    }
}

function readMessage(value: unknown, texts: string[], citations: Citation[], refusals: string[]): void {
    const item = value as { type: string };
    if (item.type !== "message") return;
    const message = value as { role: string; content: unknown[] };
    if (message.role !== "assistant") return;
    for (const part of message.content) {
        const content = part as { type: string };
        if (content.type === "refusal") {
            refusals.push((part as { refusal: string }).refusal);
        } else if (content.type === "output_text") {
            const output = part as { text: string; annotations?: unknown[] | null };
            texts.push(output.text);
            for (const annotation of output.annotations ?? []) {
                if ((annotation as { type: string }).type !== "url_citation") continue;
                const citation = annotation as { url: string; title?: string | null };
                if (!citation.url.startsWith("https://") && !citation.url.startsWith("http://")) throw invalidResponse();
                citations.push({ url: citation.url, title: citation.title ?? null });
            }
        }
    }
}

function readUsage(usage: ApiUsage | null | undefined): ResearchUsage {
    const details = usage?.server_side_tool_usage_details;
    const ticks = counter(usage?.cost_in_usd_ticks);
    return {
        inputTokens: counter(usage?.input_tokens), outputTokens: counter(usage?.output_tokens),
        totalTokens: counter(usage?.total_tokens), searchCalls: counter(details?.x_search_calls),
        postsFetched: counter(details?.x_posts_fetched), usersFetched: counter(details?.x_users_fetched),
        costDollars: ticks === null ? null : ticks / 10000000000,
    };
}

function counter(value: number | null | undefined): number | null {
    if (value === undefined || value === null) return null;
    if (!Number.isSafeInteger(value) || value < 0) throw invalidResponse();
    return value;
}

/** Map status without exposing provider response bodies or credentials.
 * @param status Failed HTTP status.
 * @param retryAfter Raw Retry-After header, if present.
 * @returns Typed error preserving HTTP status and retry metadata.
 */
export function xaiHttpError(status: number, retryAfter: string | null = null): XaiError {
    let code = "http_error";
    if (status === 400 || status === 422) code = "invalid_request";
    else if (status === 401) code = "unauthorized";
    else if (status === 402) code = "quota_exceeded";
    else if (status === 403) code = "forbidden";
    else if (status === 429) code = "rate_limited";
    return new XaiError(code, "xAI research request failed: HTTP " + status.toString(), status, retryAfter);
}

function addHandles(tool: Map<string, unknown>, field: string, handles: string[] | undefined): void {
    if (handles === undefined) return;
    if (handles.length === 0 || handles.length > 20) throw invalidArgument("handle lists must contain 1–20 handles");
    for (const handle of handles) {
        if (!/^[A-Za-z0-9_]{1,15}$/.test(handle)) throw invalidArgument("handles must be X usernames without @");
    }
    tool.set(field, handles);
}

function addDate(tool: Map<string, unknown>, field: string, value: string | undefined): void {
    if (value === undefined) return;
    try {
        if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) throw invalidArgument("dates must use YYYY-MM-DD");
        Temporal.PlainDate.from(value, { overflow: "reject" });
    } catch (cause) {
        throw invalidArgument("dates must be valid calendar dates in YYYY-MM-DD format");
    }
    tool.set(field, value);
}

function integerRange(value: number, min: number, max: number, name: string): void {
    if (!Number.isInteger(value) || value < min || value > max) throw invalidArgument(name + " is outside the supported integer range");
}

function invalidArgument(message: string): XaiError { return new XaiError("invalid_argument", message); }
function invalidResponse(): XaiError { return new XaiError("invalid_response", "xAI returned an invalid research response"); }
