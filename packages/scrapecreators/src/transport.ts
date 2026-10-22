import { ProviderIssue } from "./types";
export { ProviderIssue } from "./types";
import { Response } from "submilli:http";
import secrets from "submilli:secrets";
import { encodeComponent, parse } from "submilli:url";
/** Local failures have HTTP status 0. Permission and transport errors pass through. */
export class ScrapeCreatorsError extends Error {
    code: string;
    status: number;
    providerCode: string | number | null;
    retryAfter: string | null;
    constructor(code: string, message: string, status: number = 0, providerCode: string | number | null = null, retryAfter: string | null = null) {
        super(message);
        this.name = "ScrapeCreatorsError";
        this.code = code;
        this.status = status;
        this.providerCode = providerCode;
        this.retryAfter = retryAfter;
    }
}
interface Envelope {
    success?: boolean;
    message?: string | null;
    error?: unknown;
    code?: string | number | null;
}
/**
 * Decode a recorded or live provider envelope without credentials or I/O.
 * @param body Provider JSON response body.
 * @param status HTTP status, default 200. Local malformed responses use status 0.
 * @param retryAfter Raw Retry-After header, when present.
 * @returns Complete successful provider JSON; endpoint-specific types are checked by clients.
 */
export function decodeResponse(body: string, status: number = 200, retryAfter: string | null = null): unknown {
    let raw: unknown;
    let envelope: Envelope = {};
    try {
        raw = JSON.parse(body);
        if (raw === null || typeof raw !== "object" || Array.isArray(raw))
            throw invalidResponse("body is not a JSON object");
        envelope = raw as Envelope;
    }
    catch (cause) {
        if (status < 200 || status >= 300)
            throw providerError(status, "ScrapeCreators HTTP request failed", null, retryAfter);
        throw invalidResponse("body is not a JSON object");
    }
    if (status < 200 || status >= 300 || envelope.success === false) {
        const message = envelope.message ?? errorMessage(envelope.error);
        throw providerError(status, message, envelope.code ?? null, retryAfter);
    }
    if (envelope.success !== true)
        throw invalidResponse("success is not true");
    return raw;
}
export function readResponse(response: Response): unknown {
    return decodeResponse(response.body, response.status, response.headers.get("retry-after") ?? null);
}
export function authHeaders(): Map<string, string> {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        throw new ScrapeCreatorsError("missing_credentials", "Bind SCRAPECREATORS_API_KEY in the blueprint");
    }
    const headers = new Map<string, string>();
    headers.set("x-api-key", key);
    headers.set("Accept", "application/json");
    headers.set("Content-Type", "application/json");
    return headers;
}
export function stringParam(params: Map<string, string>, name: string, value: string | undefined, required: boolean, allowed: string[] = []): void {
    if (value === undefined) {
        if (required)
            throw invalidArgument(name + " is required");
        return;
    }
    if (typeof value !== "string" || value.trim().length === 0)
        throw invalidArgument(name + " must not be blank");
    if (allowed.length > 0 && !allowed.includes(value))
        throw invalidArgument("Unsupported " + name);
    if (name === "url")
        validateUrl(value);
    params.set(name, value);
}
export function booleanParam(params: Map<string, string>, name: string, value: boolean | undefined, required: boolean): void {
    if (value === undefined) {
        if (required)
            throw invalidArgument(name + " is required");
        return;
    }
    if (typeof value !== "boolean")
        throw invalidArgument(name + " must be boolean");
    params.set(name, value ? "true" : "false");
}
export function integerParam(params: Map<string, string>, name: string, value: number | undefined, required: boolean): void {
    if (value === undefined) {
        if (required)
            throw invalidArgument(name + " is required");
        return;
    }
    if (!Number.isSafeInteger(value) || value < 1)
        throw invalidArgument(name + " must be a positive safe integer");
    params.set(name, value.toString());
}
export function requireIdentity(values: (string | undefined)[]): void {
    for (const value of values)
        if (value !== undefined && value.trim().length > 0)
            return;
    throw invalidArgument("Provide a handle, URL, channelId, or subreddit as documented for this operation");
}
export function queryString(params: Map<string, string>): string {
    const pairs: string[] = [];
    for (const [key, value] of params)
        pairs.push(encodeComponent(key) + "=" + encodeComponent(value));
    return pairs.length === 0 ? "" : "?" + pairs.join("&");
}
export function validateDates(since: string | undefined, until: string | undefined): void {
    if (since !== undefined)
        validateDate(since);
    if (until !== undefined)
        validateDate(until);
    if (since !== undefined && until !== undefined && since > until)
        throw invalidArgument("since must not be after until");
}
export function validateSearchCursor(cursor: string | undefined): void {
    if (cursor === undefined)
        return;
    if (!/^(?:[1-9]|10|11)$/.test(cursor))
        throw invalidArgument("Search cursor must be a returned page token from 1 through 11");
}
export function redditCommentsBody(params: Map<string, string>): string {
    const fields: string[] = [];
    for (const [name, value] of params) {
        fields.push(JSON.stringify(name) + ":" + (name === "trim" ? value : JSON.stringify(value)));
    }
    return "{" + fields.join(",") + "}";
}
export function invalidArgument(message: string): ScrapeCreatorsError {
    return new ScrapeCreatorsError("invalid_argument", message);
}
export function invalidResponse(detail: string): ScrapeCreatorsError {
    return new ScrapeCreatorsError("invalid_response", "ScrapeCreators returned an invalid response: " + detail);
}
/** A failed endpoint check keeps its own error; a failed cast keeps what it expected. */
export function invalidResponseFrom(cause: unknown): ScrapeCreatorsError {
    if (cause instanceof ScrapeCreatorsError)
        return cause;
    return invalidResponse(cause instanceof Error ? cause.message : String(cause));
}
function validateUrl(url: string): void {
    try {
        const value = parse(url);
        if ((value.protocol !== "https" && value.protocol !== "http") || value.host.length === 0 ||
            /^https?:\/\/[^/?#]*@/i.test(url) || url.trim() !== url) {
            throw invalidArgument("url must be an absolute HTTP(S) URL without credentials or surrounding whitespace");
        }
    }
    catch (cause) {
        throw invalidArgument("url must be an absolute HTTP(S) URL without credentials or surrounding whitespace");
    }
}
function validateDate(value: string): void {
    if (!/^\d{4}-\d{2}-\d{2}$/.test(value))
        throw invalidArgument("Dates must use YYYY-MM-DD");
    try {
        Temporal.PlainDate.from(value);
    }
    catch (cause) {
        throw invalidArgument("Invalid calendar date");
    }
}
function errorMessage(error: unknown): string {
    if (typeof error === "string")
        return error;
    if (error !== undefined && error !== null)
        return JSON.stringify(error) ?? "ScrapeCreators reported a failure";
    return "ScrapeCreators reported a failure";
}
function providerError(status: number, message: string, providerCode: string | number | null, retryAfter: string | null): ScrapeCreatorsError {
    const text = message.toLowerCase();
    let code = "provider_error";
    if (status === 400 || status === 422)
        code = "invalid_request";
    if (status === 404 || status === 410 || text.includes("not publicly available") || text.includes("account is private"))
        code = "unavailable_content";
    if (status === 401)
        code = "unauthorized";
    if (status === 403)
        code = "forbidden";
    if (status === 402 || text.includes("insufficient credits") || text.includes("no credits") || text.includes("not enough credits") || text.includes("out of credits") || text.includes("quota"))
        code = "quota_exceeded";
    if (status === 429 || text.includes("rate limit"))
        code = "rate_limited";
    if (status >= 200 && status < 300 && (text.includes("api key") || text.includes("unauthorized")))
        code = "unauthorized";
    if (status >= 200 && status < 300 && (text.includes("not found") || text.includes("unavailable") || text.includes("no transcript")))
        code = "unavailable_content";
    return new ScrapeCreatorsError(code, message, status, providerCode, retryAfter);
}
export function responseIssues(raw: unknown): ProviderIssue[] {
    const envelope = raw as {
        errors?: unknown;
        error?: unknown;
        warnings?: unknown;
    };
    const issues: ProviderIssue[] = [];
    addIssues(issues, "error", envelope.errors);
    addIssues(issues, "error", envelope.error);
    addIssues(issues, "warning", envelope.warnings);
    return issues;
}
function addIssues(issues: ProviderIssue[], kind: "error" | "warning", value: unknown): void {
    if (value === undefined || value === null)
        return;
    const entries = Array.isArray(value) ? value as unknown[] : [value];
    for (const entry of entries) {
        let code: string | number | null = null;
        let url: string | null = null;
        let message = errorMessage(entry);
        try {
            const detail = entry as {
                code?: string | number | null;
                url?: string | null;
                message?: string | null;
            };
            code = detail.code ?? null;
            url = detail.url ?? null;
            message = detail.message ?? message;
        }
        catch (cause) {
            // Providers also return plain strings and platform-specific issue objects.
        }
        issues.push({ kind: kind, code: code, url: url, message: message, raw: entry });
    }
}
