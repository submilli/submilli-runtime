// Helpers for the hosted Stagehand browser. The API key is read from
// BROWSERBASE_API_KEY. This package never accepts or sends a project id.
// Connection URLs and signing keys are not returned.

import { parse } from "submilli:url";

/** Browserbase public API origin. Kept as a constant so HTTP host filters can be derived. */
export const BASE = "https://api.browserbase.com";

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

/** One named placeholder an agent run may substitute as `%name%`. */
export interface NamedVariable {
    /** Placeholder name, `[A-Za-z_][A-Za-z0-9_]{0,63}`. */
    name: string;
    /** Value the placeholder resolves to. Not persisted by Browserbase. */
    value: string;
    /** Optional hint shown to the agent. */
    description?: string;
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

/** The submitted URL a navigation grant can filter. Query and fragment are not included. */
export interface PageTarget {
    /** Lower-case host with no trailing dot. */
    host: string;
    /** Path, beginning with `/`. */
    path: string;
    /** `http` or `https`. */
    protocol: string;
}

/**
 * Resolve the host, path, and protocol a navigation grant must allow.
 * Rejects credentials and non-HTTP URLs. The query is not part of the grant.
 * @param url Absolute HTTP or HTTPS URL, at most 2048 characters, with no userinfo.
 * @returns The fields `browserbase.com/navigate` checks.
 */
export function pageTarget(url: string): PageTarget {
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
        const path = parsed.path.length === 0 ? "/" : parsed.path;
        return { host: parsed.host, path: path, protocol: parsed.protocol };
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidArgument("url must be an absolute HTTP or HTTPS URL");
    }
}

/**
 * Resolve the host a navigation grant must allow.
 * @param url Absolute HTTP or HTTPS URL, at most 2048 characters, with no userinfo.
 * @returns Lower-case host with no trailing dot.
 */
export function fetchHost(url: string): string {
    return pageTarget(url).host;
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
        message = "Browserbase plan quota is exhausted. The free plan includes a limited allowance of browser time and sessions, plus $5 of Model Gateway tokens. This is a quota limit, not an invalid API key";
    } else if (status === 403) {
        code = "forbidden";
        message = "Browserbase denied access; the plan may not include this feature";
    } else if (status === 404) {
        code = "not_found";
        message = "Browserbase resource was not found";
    } else if (status === 409) {
        code = "conflict";
        message = "Browserbase rejected the state change; the browser may no longer be in the required state";
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

function invalidArgument(message: string): BrowserbaseError {
    return new BrowserbaseError("invalid_argument", message);
}

function invalidResponse(): BrowserbaseError {
    return new BrowserbaseError("invalid_response", "Browserbase returned an invalid response");
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

