// Request builders, response normalizers, and errors for the Browserbase REST API.
// lib.ts re-exports the public types. Helpers stay out of the package API.
//
// The API key resolves the project. Request bodies never include projectId.
// Session connect URLs and signing keys embed that key, so normalizers drop them.

import { parse } from "submilli:url";

/** Browserbase public API origin. Kept as a constant so HTTP host filters can be derived. */
export const BASE = "https://api.browserbase.com";

const SESSION_STATUSES = ["PENDING", "RUNNING", "ERROR", "TIMED_OUT", "COMPLETED"];
const RUN_STATUSES = ["PENDING", "RUNNING", "COMPLETED", "FAILED", "STOPPED", "TIMED_OUT", "PAUSED"];
const PARALLEL_MODES = ["fast", "advanced"];
const EXA_MODES = ["fast", "deep-lite", "deep"];

/** Search stays on Parallel advanced unless the caller picks a provider and mode. */
export interface SearchOptions {
    /** Result count, 1–25. Omitted sends 10. */
    numResults?: number;
    /** `parallel` (default) or `exa`. Required when `mode` is set. */
    provider?: string;
    /** Provider mode. Parallel: `fast` or `advanced`. Exa: `fast`, `deep-lite`, or `deep`. */
    mode?: string;
}

/** One search hit. Optional metadata is null when the provider omits it. */
export interface SearchResult {
    /** Provider result id. */
    id: string;
    /** Result URL. Keep it with the title for attribution. */
    url: string;
    /** Result title. */
    title: string;
    /** Author, or null. */
    author: string | null;
    /** Publication time, or null. */
    publishedDate: string | null;
    /** Image URL, or null. */
    image: string | null;
    /** Favicon URL, or null. */
    favicon: string | null;
}

/** One search response. There is no continuation cursor. */
export interface SearchResponse {
    /** Provider request id. */
    requestId: string;
    /** Query the provider executed. */
    query: string;
    /** Hits in provider order. Empty when nothing matched. */
    results: SearchResult[];
}

/** Fetch body format. JSON schema extraction is not exposed; it can spend Model Gateway tokens. */
export type FetchFormat = "markdown" | "raw";

/** Controls for one page fetch. Omitted flags are sent as false. */
export interface FetchOptions {
    /** Follow redirects. The submitted host is not a guarantee about the final host. */
    allowRedirects?: boolean;
    /** Bypass TLS verification. Default false. */
    allowInsecureSsl?: boolean;
    /** Route the fetch through Browserbase proxies. Paid; default false. */
    proxies?: boolean;
    /** `markdown` (default) or `raw`. */
    format?: FetchFormat;
    /** Context whose cookies are sent with the request. */
    contextId?: string;
}

/** One response header from a fetch. */
export interface Header {
    /** Header name, as the fetch service returned it. */
    name: string;
    /** Header value. */
    value: string;
}

/** A fetched page. `content` is text; this package does not request JSON extraction. */
export interface FetchedPage {
    /** Fetch request id. */
    id: string;
    /** HTTP status of the fetched origin, not of the Browserbase API call. */
    statusCode: number;
    /** Response headers. */
    headers: Header[];
    /** Page text. Markdown or the raw body, matching `format`. */
    content: string;
    /** Origin content type, or empty when absent. */
    contentType: string;
    /** Origin encoding, or empty when absent. */
    encoding: string;
}

/** Viewport size in CSS pixels. */
export interface Viewport {
    /** Width, 1–8192. */
    width: number;
    /** Height, 1–8192. */
    height: number;
}

/** A cloud browser session. Connection URLs and signing keys are never included. */
export interface Session {
    /** Session id. */
    id: string;
    /** `PENDING`, `RUNNING`, `ERROR`, `TIMED_OUT`, `COMPLETED`, or a future status string. */
    status: string;
    /** Region where the session runs. */
    region: string;
    /** Whether the session survives disconnects. */
    keepAlive: boolean;
    /** Proxy bytes used. */
    proxyBytes: number;
    /** Linked context, or null. */
    contextId: string | null;
    /** Creation time. */
    createdAt: string;
    /** Last update time. */
    updatedAt: string;
    /** Start time. */
    startedAt: string;
    /** Expiry time. */
    expiresAt: string;
    /** End time, or null while the session has not ended. */
    endedAt: string | null;
}

/** One page inside a live view. */
export interface LivePage {
    /** Page id. */
    id: string;
    /** Page URL. */
    url: string;
    /** Page title. */
    title: string;
    /** Favicon URL. */
    faviconUrl: string;
    /** Live debugger URL for this page. */
    debuggerUrl: string;
    /** Fullscreen live debugger URL for this page. */
    debuggerFullscreenUrl: string;
}

/**
 * Live view links for a running session. The CDP `wsUrl` is omitted: this
 * package cannot drive a browser, and that socket is a connection secret.
 */
export interface LiveView {
    /** Fullscreen live view URL. */
    debuggerFullscreenUrl: string;
    /** Live view URL. */
    debuggerUrl: string;
    /** Open pages. */
    pages: LivePage[];
}

/** A saved browser context (cookies and login), without encryption material. */
export interface Context {
    /** Context id. */
    id: string;
    /** Caller-supplied name, or null. */
    name: string | null;
    /** Creation time. */
    createdAt: string;
    /** Last update time. */
    updatedAt: string;
}

/** The id returned when a context is created. Encryption keys are not returned. */
export interface CreatedContext {
    /** Context id. */
    id: string;
    /** Trimmed name that was stored, or null when none was sent. */
    name: string | null;
}

/** Fields for a reusable agent. */
export interface AgentOptions {
    /** System prompt applied to every run of this agent. */
    systemPrompt?: string;
    /** JSON Schema object the run result should conform to. */
    resultSchema?: unknown;
}

/** A reusable agent definition. */
export interface Agent {
    /** Agent id, passed as `agentId` to `runAgent`. */
    agentId: string;
    /** Display name. */
    name: string;
    /** System prompt, or null when unset. */
    systemPrompt: string | null;
    /** Result schema, or null when unset. */
    resultSchema: unknown;
    /** Creation time. */
    createdAt: string;
    /** Last update time. */
    updatedAt: string;
}

/** Cursor page shared by agent and run lists. */
export interface PageOptions {
    /** Page size, 1–100. Omitted sends 20. */
    limit?: number;
    /** Cursor from the previous page's `nextCursor`. */
    cursor?: string;
    /** Inclusive RFC 3339 lower bound. */
    startAt?: string;
    /** Inclusive RFC 3339 upper bound. */
    endAt?: string;
}

/** One page of agents. */
export interface AgentPage {
    /** Agents on this page. */
    agents: Agent[];
    /** Page size that was applied. */
    limit: number;
    /** Cursor for the next page, or null on the last page. */
    nextCursor: string | null;
}

/** One named placeholder an agent run may substitute as `%name%`. */
export interface NamedVariable {
    /** Placeholder name, `[A-Za-z_][A-Za-z0-9_]{0,63}`. */
    name: string;
    /** Value the placeholder resolves to. Not persisted by Browserbase. */
    value: string;
    /** Optional hint shown to the agent. */
    description?: string;
}

/** Controls for one agent run. Proxies and Verified default off. */
export interface RunOptions {
    /** Reusable agent id. Omitted starts an ad-hoc run. */
    agentId?: string;
    /** JSON Schema object for this run's result. Overrides the agent's schema. */
    resultSchema?: unknown;
    /** When the agent should pause for input. */
    pauseWhen?: string;
    /** Placeholders. Values are sent to Browserbase and are not logged here. */
    variables?: NamedVariable[];
    /** Managed proxies. Paid. Default false. */
    proxies?: boolean;
    /** Verified mode. Paid. Default false. */
    verified?: boolean;
    /** Context to load into the run's browser. */
    contextId?: string;
    /** Write cookies back to that context. Requires `contextId`. */
    persistContext?: boolean;
}

/** Why a run failed, when Browserbase reported a cause. */
export interface RunCause {
    /** Provider failure code. */
    code: string;
    /** Human-readable detail, or null. */
    message: string | null;
}

/** One agent run. Poll `getRun` until status is terminal; this package does not poll. */
export interface AgentRun {
    /** Run id. */
    runId: string;
    /** Agent id, or null for an ad-hoc run. */
    agentId: string | null;
    /** Task text. */
    task: string;
    /** Status string. Terminal values include `COMPLETED`, `FAILED`, `STOPPED`, and `TIMED_OUT`. */
    status: string;
    /** Browser session id, or null before one exists. */
    sessionId: string | null;
    /** Structured result, or null until the run finishes with output. */
    result: unknown;
    /** Failure cause, or null. */
    cause: RunCause | null;
    /** Start time, or null. */
    startedAt: string | null;
    /** End time, or null. */
    endedAt: string | null;
    /** Creation time. */
    createdAt: string;
    /** Last update time. */
    updatedAt: string;
}

/** Filters for listing runs. */
export interface RunListOptions {
    /** Page size, 1–100. Omitted sends 20. */
    limit?: number;
    /** Cursor from the previous page. */
    cursor?: string;
    /** Status filter. */
    status?: string;
    /** Only runs of this agent. */
    agentId?: string;
    /** Inclusive RFC 3339 lower bound. */
    startAt?: string;
    /** Inclusive RFC 3339 upper bound. */
    endAt?: string;
}

/** One page of runs. */
export interface RunPage {
    /** Runs on this page. */
    runs: AgentRun[];
    /** Page size that was applied. */
    limit: number;
    /** Cursor for the next page, or null on the last page. */
    nextCursor: string | null;
}

/** Page of run messages. `all` is not supported; every call sends a limit. */
export interface MessagePageOptions {
    /** Page size, 1–100. Omitted sends 20. */
    limit?: number;
    /** Id of the last message already seen. Omit on the first page. */
    since?: string;
}

/** Assistant or tool text from a run. Other tool inputs are omitted so variable values are not echoed. */
export interface RunMessage {
    /** Message id. Pass the last id as `since` to continue. */
    id: string;
    /** Creation time. */
    createdAt: string;
    /** `assistant` or `tool`, or another role the provider sends. */
    role: string;
    /** Text parts, plus a pause tool's `input.message`. Empty when the message has neither. */
    text: string;
}

/** One page of run messages, oldest first. */
export interface MessagePage {
    /** Messages on this page. */
    messages: RunMessage[];
    /** Id to pass as `since` for the next page, or null when the run has no messages yet. */
    nextSince: string | null;
}

/** Values already read from the caller for one fetch. */
export interface FetchRequest {
    allowRedirects: boolean;
    allowInsecureSsl: boolean;
    proxies: boolean;
    format: FetchFormat;
    contextId: string;
}

/** Values already read from the caller for one search. */
export interface SearchRequest {
    numResults: number;
    provider: string;
    mode: string;
}

/** API or validation failure. Local failures use status 0. No automatic retries. */
export class BrowserbaseError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;

    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "BrowserbaseError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/**
 * Build a search body. Provider and mode are always sent so a grant can see them.
 * @param query Search text, 1–200 characters.
 * @param options Normalized search controls.
 * @returns JSON request body.
 */
export function buildSearchBody(query: string, options: SearchRequest): string {
    requireLength(query, 1, 200, "query");
    integerRange(options.numResults, 1, 25, "numResults");
    requireSearchMode(options.provider, options.mode);
    return objectJson([
        field("query", JSON.stringify(query)),
        field("numResults", JSON.stringify(options.numResults)),
        field("provider", JSON.stringify(options.provider)),
        field("mode", JSON.stringify(options.mode)),
    ]);
}

/**
 * Resolve the host a fetch grant must allow, and reject credentials or non-HTTP URLs.
 * @param url Absolute HTTP or HTTPS URL, at most 2048 characters, with no userinfo.
 * @returns Lower-case host with no trailing dot, the spelling `http` rules see.
 */
export function fetchHost(url: string): string {
    if (url.length > 2048 || url.trim() !== url) {
        throw invalidArgument("url must be at most 2048 characters with no surrounding whitespace");
    }
    if (authorityHasUserinfo(url)) {
        throw invalidArgument("url must not contain credentials");
    }
    try {
        const parsed = parse(url);
        if ((parsed.protocol !== "https" && parsed.protocol !== "http") || /^\.*$/.test(parsed.host)) {
            throw invalidArgument("url must be an absolute HTTP or HTTPS URL");
        }
        return parsed.host;
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidArgument("url must be an absolute HTTP or HTTPS URL");
    }
}

/**
 * Build a fetch body. Flags are explicit so the request matches the capability check.
 * @param url Absolute URL, already validated by `fetchHost`.
 * @param options Normalized fetch controls.
 * @returns JSON request body. Omits `contextId` when it is empty.
 */
export function buildFetchBody(url: string, options: FetchRequest): string {
    fetchHost(url);
    if (options.format !== "markdown" && options.format !== "raw") {
        throw invalidArgument("format must be markdown or raw");
    }
    const fields = [
        field("url", JSON.stringify(url)),
        field("allowRedirects", JSON.stringify(options.allowRedirects)),
        field("allowInsecureSsl", JSON.stringify(options.allowInsecureSsl)),
        field("proxies", JSON.stringify(options.proxies)),
        field("format", JSON.stringify(options.format)),
    ];
    if (options.contextId !== "") {
        fields.push(field("contextId", JSON.stringify(resourceId(options.contextId, "contextId"))));
    }
    return objectJson(fields);
}

/**
 * Accept a Browserbase resource id and reject path segments.
 * @param value Id to validate.
 * @param name Argument name used in the error.
 * @returns The same id when it matches `[A-Za-z0-9_-]{1,128}`.
 */
export function resourceId(value: string, name: string): string {
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(value)) {
        throw invalidArgument(name + " must be 1–128 characters of letters, digits, underscores, or hyphens");
    }
    return value;
}

/**
 * Validate a session status filter.
 * @param status Status to send.
 * @returns The same status when it is a known session status.
 */
export function sessionStatus(status: string): string {
    if (!SESSION_STATUSES.includes(status)) {
        throw invalidArgument("status must be PENDING, RUNNING, ERROR, TIMED_OUT, or COMPLETED");
    }
    return status;
}

/**
 * Validate a run status filter.
 * @param status Status to send.
 * @returns The same status when it is a known run status.
 */
export function runStatus(status: string): string {
    if (!RUN_STATUSES.includes(status)) {
        throw invalidArgument("status must be PENDING, RUNNING, COMPLETED, FAILED, STOPPED, TIMED_OUT, or PAUSED");
    }
    return status;
}

/**
 * Build a context create body. The name is trimmed. projectId is never sent.
 * @param name Optional context name, 1–128 characters after trimming.
 * @returns JSON request body, `{}` when no name was given.
 */
export function buildContextBody(name: string | undefined): string {
    if (name === undefined) return "{}";
    const trimmed = name.trim();
    requireLength(trimmed, 1, 128, "name");
    return objectJson([field("name", JSON.stringify(trimmed))]);
}

/**
 * The name that will be stored, or null when the caller omitted one.
 * @param name Optional context name.
 * @returns Trimmed name, or null.
 */
export function contextName(name: string | undefined): string | null {
    if (name === undefined) return null;
    return name.trim();
}

/**
 * Build an agent create or the fields of an update. Snapshots `resultSchema` before return.
 * @param name Agent name, 1–255 characters. Pass undefined only for a partial update body.
 * @param options Prompt and schema. Schema must be a JSON object.
 * @returns JSON request body.
 */
export function buildAgentBody(name: string | undefined, options: AgentOptions): string {
    const fields: string[] = [];
    if (name !== undefined) {
        requireLength(name, 1, 255, "name");
        fields.push(field("name", JSON.stringify(name)));
    }
    if (options.systemPrompt !== undefined) {
        requireLength(options.systemPrompt, 1, 8000, "systemPrompt");
        fields.push(field("systemPrompt", JSON.stringify(options.systemPrompt)));
    }
    if (options.resultSchema !== undefined) {
        fields.push(field("resultSchema", schemaJson(options.resultSchema)));
    }
    if (fields.length === 0) throw invalidArgument("agent update needs a name, systemPrompt, or resultSchema");
    return objectJson(fields);
}

/** Normalized run controls. */
export interface RunRequest {
    task: string;
    agentId: string;
    proxies: boolean;
    verified: boolean;
    contextId: string;
    persistContext: boolean;
    pauseWhen?: string;
    variables?: NamedVariable[];
    resultSchema?: unknown;
}

/**
 * Normalize a run and copy variables so later reads cannot change the check.
 * @param task Task text, 1–8000 characters.
 * @param options Caller options, already destructured by the caller.
 * @returns Values the request will send.
 */
export function runRequest(task: string, options: RunOptions): RunRequest {
    requireLength(task, 1, 8000, "task");
    const agentId = options.agentId ?? "";
    if (agentId !== "") resourceId(agentId, "agentId");
    const contextId = options.contextId ?? "";
    if (contextId !== "") resourceId(contextId, "contextId");
    const persistContext = options.persistContext ?? false;
    if (persistContext && contextId === "") throw invalidArgument("persistContext requires contextId");
    const pauseWhen = options.pauseWhen;
    if (pauseWhen !== undefined) requireLength(pauseWhen, 1, 2000, "pauseWhen");
    const variables = options.variables === undefined ? undefined : copyVariables(options.variables);
    const resultSchema = options.resultSchema;
    if (resultSchema !== undefined) schemaJson(resultSchema);
    return {
        task: task,
        agentId: agentId,
        proxies: options.proxies ?? false,
        verified: options.verified ?? false,
        contextId: contextId,
        persistContext: persistContext,
        pauseWhen: pauseWhen,
        variables: variables,
        resultSchema: resultSchema,
    };
}

/**
 * Build a run create body. Proxies and Verified are explicit booleans, never proxy configs.
 * @param request Normalized run controls.
 * @returns JSON request body.
 */
export function buildRunBody(request: RunRequest): string {
    const fields = [field("task", JSON.stringify(request.task))];
    if (request.agentId !== "") fields.push(field("agentId", JSON.stringify(request.agentId)));
    if (request.resultSchema !== undefined) fields.push(field("resultSchema", schemaJson(request.resultSchema)));
    if (request.pauseWhen !== undefined) fields.push(field("pauseWhen", JSON.stringify(request.pauseWhen)));
    if (request.variables !== undefined) fields.push(field("variables", variablesJson(request.variables)));
    fields.push(field("browserSettings", browserSettingsJson(request.proxies, request.verified, request.contextId, request.persistContext)));
    return objectJson(fields);
}

/**
 * Build a resume body. The task is the reply to the agent's pause.
 * @param task Reply text, 1–8000 characters.
 * @param variables Optional placeholders, already copied.
 * @returns JSON request body.
 */
export function buildResumeBody(task: string, variables: NamedVariable[] | undefined): string {
    requireLength(task, 1, 8000, "task");
    const fields = [field("task", JSON.stringify(task))];
    if (variables !== undefined) fields.push(field("variables", variablesJson(variables)));
    return objectJson(fields);
}

/**
 * Copy variables once. Values are not included in errors.
 * @param variables Caller variables.
 * @returns A package-owned copy, at most 20 entries.
 */
export function copyVariables(variables: NamedVariable[]): NamedVariable[] {
    integerRange(variables.length, 0, 20, "variables count");
    const copied: NamedVariable[] = [];
    for (const variable of variables) {
        if (!/^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(variable.name)) {
            throw invalidArgument("variable name must match [A-Za-z_][A-Za-z0-9_]{0,63}");
        }
        if (variable.value.length > 4000) throw invalidArgument("variable value must be at most 4000 characters");
        const entry: NamedVariable = { name: variable.name, value: variable.value };
        if (variable.description !== undefined) {
            requireLength(variable.description, 1, 500, "variable description");
            entry.description = variable.description;
        }
        copied.push(entry);
    }
    return copied;
}

/** Normalized list query. */
export interface ListQuery {
    limit: number;
    cursor?: string;
    startAt?: string;
    endAt?: string;
    status?: string;
    agentId?: string;
}

/**
 * Validate list controls and return a query string including the leading `?`.
 * Always includes `limit`.
 * @param query Normalized list controls.
 * @returns Query string, never empty.
 */
export function listQuery(query: ListQuery): string {
    integerRange(query.limit, 1, 100, "limit");
    const params = new Map<string, string>();
    params.set("limit", query.limit.toString());
    if (query.cursor !== undefined) params.set("cursor", cursorValue(query.cursor));
    if (query.status !== undefined) params.set("status", query.status);
    if (query.agentId !== undefined) params.set("agentId", resourceId(query.agentId, "agentId"));
    addInstant(params, "startAt", query.startAt);
    addInstant(params, "endAt", query.endAt);
    if (query.startAt !== undefined && query.endAt !== undefined &&
        Temporal.Instant.from(query.startAt).epochMilliseconds > Temporal.Instant.from(query.endAt).epochMilliseconds) {
        throw invalidArgument("startAt must not be after endAt");
    }
    return "?" + encodeParams(params);
}

/**
 * Validate a message page and return its query string.
 * @param limit Page size, 1–100.
 * @param since Optional message id to continue from.
 * @returns Query string including the leading `?`.
 */
export function messageQuery(limit: number, since: string | undefined): string {
    integerRange(limit, 1, 100, "limit");
    const params = new Map<string, string>();
    params.set("limit", limit.toString());
    if (since !== undefined) params.set("since", cursorValue(since));
    return "?" + encodeParams(params);
}

/**
 * Validate a live-view TTL.
 * @param expiresIn Seconds, 60–21600.
 * @returns Query string including the leading `?`.
 */
export function liveQuery(expiresIn: number): string {
    integerRange(expiresIn, 60, 21600, "expiresIn");
    const params = new Map<string, string>();
    params.set("expiresIn", expiresIn.toString());
    return "?" + encodeParams(params);
}

/**
 * Normalize a search response.
 * @param body Raw JSON body.
 * @returns Parsed hits. Throws `BrowserbaseError` with code `invalid_response` when the body is malformed.
 */
export function normalizeSearchJson(body: string): SearchResponse {
    try {
        const data = JSON.parse(body) as ApiSearch;
        if (data.requestId.length === 0 || data.query.length === 0) throw invalidResponse();
        const results: SearchResult[] = [];
        for (const item of data.results) {
            if (item.id.length === 0 || item.url.trim().length === 0 || item.title.length === 0) throw invalidResponse();
            results.push({
                id: item.id,
                url: item.url,
                title: item.title,
                author: item.author ?? null,
                publishedDate: item.publishedDate ?? null,
                image: item.image ?? null,
                favicon: item.favicon ?? null,
            });
        }
        return { requestId: data.requestId, query: data.query, results: results };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize a fetch response. Content must be text because this package requests markdown or raw.
 * @param body Raw JSON body.
 * @returns The fetched page.
 */
export function normalizeFetchJson(body: string): FetchedPage {
    try {
        const data = JSON.parse(body) as ApiFetch;
        if (data.id.length === 0 || typeof data.content !== "string") throw invalidResponse();
        if (!Number.isFinite(data.statusCode)) throw invalidResponse();
        return {
            id: data.id,
            statusCode: data.statusCode,
            headers: headersFrom(data.headers),
            content: data.content,
            contentType: data.contentType ?? "",
            encoding: data.encoding ?? "",
        };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize one session, dropping connection secrets.
 * @param body Raw JSON body.
 * @returns The session without connect URL, Selenium URL, or signing key.
 */
export function normalizeSessionJson(body: string): Session {
    try {
        return sessionFrom(JSON.parse(body) as ApiSession);
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize a session list.
 * @param body Raw JSON body, a JSON array.
 * @returns Sessions in provider order.
 */
export function normalizeSessionListJson(body: string): Session[] {
    try {
        const data = JSON.parse(body) as ApiSession[];
        const sessions: Session[] = [];
        for (const item of data) sessions.push(sessionFrom(item));
        return sessions;
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize live view links and refuse any URL that carries an API key.
 * @param body Raw JSON body.
 * @returns Debugger links and pages. `wsUrl` is not returned.
 */
export function normalizeLiveViewJson(body: string): LiveView {
    try {
        const data = JSON.parse(body) as ApiLive;
        const pages: LivePage[] = [];
        for (const page of data.pages) {
            if (page.id.length === 0) throw invalidResponse();
            const view: LivePage = {
                id: page.id,
                url: page.url,
                title: page.title,
                faviconUrl: page.faviconUrl,
                debuggerUrl: page.debuggerUrl,
                debuggerFullscreenUrl: page.debuggerFullscreenUrl,
            };
            rejectApiKey(view.debuggerUrl);
            rejectApiKey(view.debuggerFullscreenUrl);
            pages.push(view);
        }
        rejectApiKey(data.debuggerUrl);
        rejectApiKey(data.debuggerFullscreenUrl);
        return { debuggerFullscreenUrl: data.debuggerFullscreenUrl, debuggerUrl: data.debuggerUrl, pages: pages };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize a context. The public encryption key is not returned.
 * @param body Raw JSON body.
 * @returns Context id, name, and timestamps.
 */
export function normalizeContextJson(body: string): Context {
    try {
        const data = JSON.parse(body) as ApiContext;
        if (data.id.length === 0 || data.createdAt.length === 0 || data.updatedAt.length === 0) throw invalidResponse();
        return { id: data.id, name: data.name ?? null, createdAt: data.createdAt, updatedAt: data.updatedAt };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Read the id from a context create response. Encryption material is ignored.
 * @param body Raw JSON body.
 * @returns The new context id.
 */
export function createdContextId(body: string): string {
    try {
        const data = JSON.parse(body) as { id?: string };
        if (data.id === undefined || data.id.length === 0) throw invalidResponse();
        return data.id;
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize an agent.
 * @param body Raw JSON body.
 * @returns The agent definition.
 */
export function normalizeAgentJson(body: string): Agent {
    try {
        return agentFrom(JSON.parse(body) as ApiAgent);
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize one page of agents.
 * @param body Raw JSON body.
 * @returns Agents and the next cursor.
 */
export function normalizeAgentPageJson(body: string): AgentPage {
    try {
        const data = JSON.parse(body) as ApiAgentPage;
        const agents: Agent[] = [];
        for (const item of data.data) agents.push(agentFrom(item));
        return { agents: agents, limit: data.limit, nextCursor: data.nextCursor ?? null };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize an agent run.
 * @param body Raw JSON body.
 * @returns The run. Connection URLs are not part of this schema and are not returned.
 */
export function normalizeRunJson(body: string): AgentRun {
    try {
        return runFrom(JSON.parse(body) as ApiRun);
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize one page of runs.
 * @param body Raw JSON body.
 * @returns Runs and the next cursor.
 */
export function normalizeRunPageJson(body: string): RunPage {
    try {
        const data = JSON.parse(body) as ApiRunPage;
        const runs: AgentRun[] = [];
        for (const item of data.data) runs.push(runFrom(item));
        return { runs: runs, limit: data.limit, nextCursor: data.nextCursor ?? null };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Normalize run messages. Text parts are kept. A pause tool's question is kept.
 * Other tool inputs are dropped so variable values are not echoed.
 * @param body Raw JSON body.
 * @returns Messages in chronological order.
 */
export function normalizeMessagesJson(body: string): MessagePage {
    try {
        const data = JSON.parse(body) as ApiMessages;
        const messages: RunMessage[] = [];
        for (const item of data.data) {
            if (item.id.length === 0 || item.createdAt.length === 0) throw invalidResponse();
            const role = item.message.role;
            if (role.length === 0) throw invalidResponse();
            messages.push({ id: item.id, createdAt: item.createdAt, role: role, text: messageText(item.message.content) });
        }
        return { messages: messages, nextSince: data.nextSince ?? null };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Map an HTTP failure without including the response body or request headers.
 * @param status HTTP status.
 * @param retryAfter `Retry-After` header, or null.
 * @returns A `BrowserbaseError` whose code distinguishes auth, quota, conflict, and rate limits.
 */
export function browserbaseHttpError(status: number, retryAfter: string | null = null): BrowserbaseError {
    let code = "http_error";
    let message = "Browserbase request failed: HTTP " + status.toString();
    if (status === 400 || status === 422) {
        code = "invalid_request";
        message = "Browserbase rejected the request; check the arguments";
    } else if (status === 401) {
        code = "unauthorized";
        message = "Browserbase rejected the API key; check the bound credential";
    } else if (status === 402) {
        code = "quota_exceeded";
        message = "Browserbase plan quota is exhausted. The free plan includes a limited allowance of browser time, sessions, and Search/Fetch calls, plus $5 of Model Gateway tokens for agent runs. This is a quota limit, not an invalid API key";
    } else if (status === 403) {
        code = "forbidden";
        message = "Browserbase denied access; the plan may not include this feature";
    } else if (status === 404) {
        code = "not_found";
        message = "Browserbase resource was not found";
    } else if (status === 409) {
        code = "conflict";
        message = "Browserbase rejected the state change; the run may no longer be in the required state";
    } else if (status === 429) {
        code = "rate_limited";
        message = "Browserbase rate limit exceeded; retry later";
    }
    return new BrowserbaseError(code, message, status, retryAfter);
}

function sessionFrom(item: ApiSession): Session {
    if (item.id.length === 0 || item.status.length === 0 || item.region.length === 0) throw invalidResponse();
    if (item.createdAt.length === 0 || item.updatedAt.length === 0 || item.startedAt.length === 0 || item.expiresAt.length === 0) {
        throw invalidResponse();
    }
    if (!Number.isFinite(item.proxyBytes)) throw invalidResponse();
    rejectApiKey(item.id);
    return {
        id: item.id,
        status: item.status,
        region: item.region,
        keepAlive: item.keepAlive,
        proxyBytes: item.proxyBytes,
        contextId: item.contextId ?? null,
        createdAt: item.createdAt,
        updatedAt: item.updatedAt,
        startedAt: item.startedAt,
        expiresAt: item.expiresAt,
        endedAt: item.endedAt ?? null,
    };
}

function agentFrom(item: ApiAgent): Agent {
    if (item.agentId.length === 0 || item.name.length === 0 || item.createdAt.length === 0 || item.updatedAt.length === 0) {
        throw invalidResponse();
    }
    return {
        agentId: item.agentId,
        name: item.name,
        systemPrompt: item.systemPrompt ?? null,
        resultSchema: item.resultSchema === undefined ? null : item.resultSchema,
        createdAt: item.createdAt,
        updatedAt: item.updatedAt,
    };
}

function runFrom(item: ApiRun): AgentRun {
    if (item.runId.length === 0 || item.task.length === 0 || item.status.length === 0) throw invalidResponse();
    if (item.createdAt.length === 0 || item.updatedAt.length === 0) throw invalidResponse();
    let cause: RunCause | null = null;
    if (item.cause !== undefined && item.cause !== null) {
        if (item.cause.code.length === 0) throw invalidResponse();
        cause = { code: item.cause.code, message: item.cause.message ?? null };
    }
    return {
        runId: item.runId,
        agentId: item.agentId ?? null,
        task: item.task,
        status: item.status,
        sessionId: item.sessionId ?? null,
        result: item.result === undefined ? null : item.result,
        cause: cause,
        startedAt: item.startedAt ?? null,
        endedAt: item.endedAt ?? null,
        createdAt: item.createdAt,
        updatedAt: item.updatedAt,
    };
}

function headersFrom(value: unknown): Header[] {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw invalidResponse();
    const headers: Header[] = [];
    for (const [name, headerValue] of Object.entries(value as Record<string, unknown>)) {
        if (typeof headerValue !== "string") throw invalidResponse();
        headers.push({ name: name, value: headerValue });
    }
    return headers;
}

function messageText(content: unknown): string {
    if (typeof content === "string") return content;
    if (!Array.isArray(content)) return "";
    const parts: string[] = [];
    for (const part of content) {
        if (part === null || typeof part !== "object") continue;
        const record = part as { text?: unknown; type?: unknown; toolName?: unknown; input?: unknown };
        if (typeof record.text === "string" && record.text.length > 0) parts.push(record.text);
        if (record.type === "tool-call" && record.toolName === "pause") {
            const input = record.input;
            if (input !== null && typeof input === "object" && !Array.isArray(input)) {
                const message = (input as { message?: unknown }).message;
                if (typeof message === "string" && message.length > 0) parts.push(message);
            }
        }
    }
    return parts.join("\n");
}

function browserSettingsJson(proxies: boolean, verified: boolean, contextId: string, persistContext: boolean): string {
    const fields = [
        field("proxies", JSON.stringify(proxies)),
        field("verified", JSON.stringify(verified)),
    ];
    if (contextId !== "") {
        fields.push(field("context", objectJson([
            field("id", JSON.stringify(contextId)),
            field("persist", JSON.stringify(persistContext)),
        ])));
    }
    return objectJson(fields);
}

function variablesJson(variables: NamedVariable[]): string {
    const fields: string[] = [];
    for (const variable of variables) {
        const entry = [field("value", JSON.stringify(variable.value))];
        if (variable.description !== undefined) entry.push(field("description", JSON.stringify(variable.description)));
        fields.push(field(variable.name, objectJson(entry)));
    }
    return objectJson(fields);
}

function schemaJson(schema: unknown): string {
    const encoded = JSON.stringify(schema);
    if (encoded === undefined || !encoded.startsWith("{")) throw invalidArgument("resultSchema must be a JSON object");
    return encoded;
}

function requireSearchMode(provider: string, mode: string): void {
    if (provider === "parallel") {
        if (!PARALLEL_MODES.includes(mode)) throw invalidArgument("parallel mode must be fast or advanced");
        return;
    }
    if (provider === "exa") {
        if (!EXA_MODES.includes(mode)) throw invalidArgument("exa mode must be fast, deep-lite, or deep");
        return;
    }
    throw invalidArgument("provider must be parallel or exa");
}

function addInstant(params: Map<string, string>, name: string, value: string | undefined): void {
    if (value === undefined) return;
    try {
        Temporal.Instant.from(value);
    } catch (cause) {
        throw invalidArgument(name + " must be an RFC 3339 timestamp");
    }
    params.set(name, value);
}

function cursorValue(value: string): string {
    if (value.trim() !== value || value.length === 0 || value.length > 2048) {
        throw invalidArgument("cursor must be 1–2048 characters with no surrounding whitespace");
    }
    return value;
}

function encodeParams(params: Map<string, string>): string {
    const parts: string[] = [];
    for (const [name, value] of params) {
        parts.push(encodeURIComponent(name) + "=" + encodeURIComponent(value));
    }
    return parts.join("&");
}

function rejectApiKey(value: string): void {
    let current = stripUrlIgnored(value);
    for (let pass = 0; pass < 4; pass++) {
        if (embedsSecretParam(current)) throw invalidResponse();
        const decoded = stripUrlIgnored(decodePercentTolerant(current));
        if (decoded === current) return;
        current = decoded;
    }
    if (embedsSecretParam(current)) throw invalidResponse();
}

function embedsSecretParam(value: string): boolean {
    const lower = value.toLowerCase();
    return lower.includes("apikey=") || lower.includes("signingkey=");
}

function stripUrlIgnored(value: string): string {
    let out = "";
    for (let index = 0; index < value.length; index++) {
        const code = value.charCodeAt(index);
        if (code === 9 || code === 10 || code === 13) continue;
        out = out + value.charAt(index);
    }
    return out;
}

function decodePercentTolerant(value: string): string {
    let out = "";
    let index = 0;
    while (index < value.length) {
        if (value.charAt(index) === "%" && index + 2 < value.length) {
            const high = hexValue(value.charAt(index + 1));
            const low = hexValue(value.charAt(index + 2));
            if (high >= 0 && low >= 0) {
                out = out + String.fromCharCode(high * 16 + low);
                index = index + 3;
                continue;
            }
        }
        out = out + value.charAt(index);
        index = index + 1;
    }
    return out;
}

function hexValue(char: string): number {
    const code = char.charCodeAt(0);
    if (code >= 48 && code <= 57) return code - 48;
    if (code >= 65 && code <= 70) return code - 55;
    if (code >= 97 && code <= 102) return code - 87;
    return -1;
}

/**
 * Whether `url.parse` would read a username or password. It drops tabs and
 * line breaks, trims leading controls, and treats `\` as `/` in an HTTP
 * authority, so `https:user:secret@example.com/` is still `example.com`.
 * An `@` after the path, query, or fragment is not userinfo.
 * @param url Caller URL, not yet parsed.
 * @returns True when the authority contains `@`.
 */
function authorityHasUserinfo(url: string): boolean {
    let stripped = "";
    for (let index = 0; index < url.length; index++) {
        const code = url.charCodeAt(index);
        if (code === 9 || code === 10 || code === 13) continue;
        stripped = stripped + url.charAt(index);
    }
    let start = 0;
    let end = stripped.length;
    while (start < end && stripped.charCodeAt(start) <= 32) start = start + 1;
    while (end > start && stripped.charCodeAt(end - 1) <= 32) end = end - 1;
    let scheme = "";
    let cursor = start;
    while (cursor < end) {
        const code = stripped.charCodeAt(cursor);
        const letter = (code >= 65 && code <= 90) || (code >= 97 && code <= 122);
        const mark = scheme.length > 0 && ((code >= 48 && code <= 57) || code === 43 || code === 45 || code === 46);
        if (!letter && !mark) break;
        scheme = scheme + stripped.charAt(cursor);
        cursor = cursor + 1;
    }
    const schemeName = scheme.toLowerCase();
    if ((schemeName !== "http" && schemeName !== "https") || cursor >= end || stripped.charCodeAt(cursor) !== 58) return false;
    cursor = cursor + 1;
    while (cursor < end) {
        const code = stripped.charCodeAt(cursor);
        if (code !== 47 && code !== 92) break;
        cursor = cursor + 1;
    }
    while (cursor < end) {
        const code = stripped.charCodeAt(cursor);
        if (code === 47 || code === 92 || code === 63 || code === 35) return false;
        if (code === 64) return true;
        cursor = cursor + 1;
    }
    return false;
}

function integerRange(value: number, min: number, max: number, name: string): void {
    if (!Number.isFinite(value) || value !== Math.floor(value) || value < min || value > max) {
        throw invalidArgument(name + " must be an integer between " + min.toString() + " and " + max.toString());
    }
}

function requireLength(value: string, min: number, max: number, name: string): void {
    const length = value.trim().length;
    if (value.trim() !== value || length < min || value.length > max) {
        throw invalidArgument(name + " must be " + min.toString() + "–" + max.toString() + " characters with no surrounding whitespace");
    }
}

function field(name: string, json: string): string {
    return JSON.stringify(name) + ":" + json;
}

function objectJson(fields: string[]): string {
    return "{" + fields.join(",") + "}";
}

function invalidArgument(message: string): BrowserbaseError {
    return new BrowserbaseError("invalid_argument", message);
}

function invalidResponse(): BrowserbaseError {
    return new BrowserbaseError("invalid_response", "Browserbase returned an invalid response");
}

interface ApiSearchResult {
    id: string;
    url: string;
    title: string;
    author?: string | null;
    publishedDate?: string | null;
    image?: string | null;
    favicon?: string | null;
}

interface ApiSearch {
    requestId: string;
    query: string;
    results: ApiSearchResult[];
}

interface ApiFetch {
    id: string;
    statusCode: number;
    headers: unknown;
    content: unknown;
    contentType?: string | null;
    encoding?: string | null;
}

interface ApiSession {
    id: string;
    createdAt: string;
    updatedAt: string;
    status: string;
    proxyBytes: number;
    keepAlive: boolean;
    region: string;
    startedAt: string;
    expiresAt: string;
    endedAt?: string | null;
    contextId?: string | null;
}

interface ApiLivePage {
    id: string;
    url: string;
    title: string;
    faviconUrl: string;
    debuggerUrl: string;
    debuggerFullscreenUrl: string;
}

interface ApiLive {
    debuggerFullscreenUrl: string;
    debuggerUrl: string;
    pages: ApiLivePage[];
}

interface ApiContext {
    id: string;
    createdAt: string;
    updatedAt: string;
    name?: string | null;
}

interface ApiAgent {
    agentId: string;
    name: string;
    systemPrompt?: string | null;
    resultSchema?: unknown;
    createdAt: string;
    updatedAt: string;
}

interface ApiAgentPage {
    data: ApiAgent[];
    limit: number;
    nextCursor?: string | null;
}

interface ApiCause {
    code: string;
    message?: string | null;
}

interface ApiRun {
    runId: string;
    agentId?: string | null;
    task: string;
    status: string;
    sessionId?: string | null;
    result?: unknown;
    cause?: ApiCause | null;
    startedAt?: string | null;
    endedAt?: string | null;
    createdAt: string;
    updatedAt: string;
}

interface ApiRunPage {
    data: ApiRun[];
    limit: number;
    nextCursor?: string | null;
}

interface ApiMessage {
    id: string;
    createdAt: string;
    message: { role: string; content: unknown };
}

interface ApiMessages {
    data: ApiMessage[];
    nextSince?: string | null;
}
