// Hosted Stagehand session API (act / extract / observe). Submilli cannot drive
// CDP, so these calls go to api.stagehand.browserbase.com. The response CDP URL
// embeds a signing key and is never returned. projectId is never sent.

import { BrowserbaseError, NamedVariable, Viewport, copyVariables, resourceId } from "./internal";

/**
 * Stagehand API prefixes. Each includes `/v1/` so a call to `prefix + suffix`
 * derives an HTTP host filter. A session in another region is not reachable
 * on the us-west-2 host, and cleanup would leave that browser running.
 */
export const STAGEHAND_US_WEST = "https://api.stagehand.browserbase.com/v1/";
export const STAGEHAND_US_EAST = "https://api.use1.stagehand.browserbase.com/v1/";
export const STAGEHAND_EU_CENTRAL = "https://api.euc1.stagehand.browserbase.com/v1/";
export const STAGEHAND_AP_SOUTHEAST = "https://api.apse1.stagehand.browserbase.com/v1/";

/** Model Gateway model used when the caller does not pin one. No provider key is sent. */
export const DEFAULT_MODEL = "google/gemini-2.5-flash";

const WAIT_UNTIL = ["load", "domcontentloaded", "networkidle"];

/** Controls for starting a Stagehand browser. Paid flags default off. */
export interface StagehandOptions {
    /** Model Gateway model id, `provider/name`. Omitted sends `google/gemini-2.5-flash`. */
    model?: string;
    /** Re-infer a failed action once. Omitted sends true. */
    selfHeal?: boolean;
    /** Milliseconds to wait for the DOM to settle, 0–30000. Omitted sends 5000. */
    domSettleTimeoutMs?: number;
    /** Region. Omitted sends `us-west-2`. */
    region?: string;
    /** Session lifetime in seconds, 60–21600. Omitted sends 300. */
    timeoutSeconds?: number;
    /** Keep the browser after disconnect. Default false. */
    keepAlive?: boolean;
    /** Managed proxies. Paid. Default false. */
    proxies?: boolean;
    /** Verified mode. Paid. Default false. */
    verified?: boolean;
    /** Automatic CAPTCHA solving. Not on the free plan. Default false. */
    solveCaptchas?: boolean;
    /** Context to load. */
    contextId?: string;
    /** Write cookies back to that context. Requires `contextId`. */
    persistContext?: boolean;
    /** Optional viewport. */
    viewport?: Viewport;
}

/** Normalized start request. */
export interface StagehandStart {
    model: string;
    selfHeal: boolean;
    domSettleTimeoutMs: number;
    region: string;
    timeoutSeconds: number;
    keepAlive: boolean;
    proxies: boolean;
    verified: boolean;
    solveCaptchas: boolean;
    contextId: string;
    persistContext: boolean;
    viewport?: Viewport;
}

/** Navigation wait condition. */
export type WaitUntil = "load" | "domcontentloaded" | "networkidle";

/** Optional navigation controls. */
export interface GotoOptions {
    /** When to consider navigation finished. Omitted sends `domcontentloaded`. */
    waitUntil?: WaitUntil;
    /** Navigation timeout in milliseconds, 1–120000. */
    timeoutMs?: number;
}

/** An action `observe` planned. Pass it back to `act` to replay it without another model call. */
export interface ObservedAction {
    /** Selector or XPath. */
    selector: string;
    /** What the action does. */
    description: string;
    /** Method such as `click` or `fill`, or null when the provider omitted it. */
    method: string | null;
    /** Method arguments. Empty when none were returned. */
    arguments: string[];
    /** Backend node id, or null. */
    backendNodeId: number | null;
}

/** Options for one `act` call. */
export interface ActOptions {
    /** Placeholders substituted as `%name%`. Values are sent to Stagehand and are not logged here. */
    variables?: NamedVariable[];
    /** Action timeout in milliseconds, 1–120000. */
    timeoutMs?: number;
}

/** Result of one `act`. `success` false is still a completed call; it is not thrown. */
export interface ActResult {
    /** Whether Stagehand reported the action as successful. */
    success: boolean;
    /** Provider message. */
    message: string;
    /** Short description of what ran. */
    actionDescription: string;
    /** Actions Stagehand executed. */
    actions: ObservedAction[];
    /** Provider action id, or null. */
    actionId: string | null;
}

/** Options for `observe`. */
export interface ObserveOptions {
    /** CSS selector that scopes the observation. */
    selector?: string;
    /** Selectors whose subtrees are excluded. */
    ignoreSelectors?: string[];
    /** Observation timeout in milliseconds, 1–120000. */
    timeoutMs?: number;
}

/** Options for `extract`. */
export interface ExtractOptions {
    /** CSS selector that scopes extraction. */
    selector?: string;
    /** Selectors whose subtrees are excluded. */
    ignoreSelectors?: string[];
    /** Include a viewport screenshot in the model call. Default false. */
    screenshot?: boolean;
    /** Extraction timeout in milliseconds, 1–120000. */
    timeoutMs?: number;
}

/** Extracted data plus the provider action id. */
export interface ExtractResult {
    /** Data matching the schema, or the provider's untyped result. */
    data: unknown;
    /** Provider action id, or null. */
    actionId: string | null;
}

/**
 * Normalize start options. Model Gateway is used; a provider API key cannot be supplied here.
 * @param options Caller options, already destructured.
 * @returns Values the start request will send.
 */
export function stagehandStart(options: StagehandOptions): StagehandStart {
    const model = options.model ?? DEFAULT_MODEL;
    if (!/^[a-z0-9][a-z0-9._-]{0,40}\/[A-Za-z0-9][A-Za-z0-9._-]{0,80}$/.test(model)) {
        throw invalidArgument("model must be a Model Gateway id such as google/gemini-2.5-flash");
    }
    const region = options.region ?? "us-west-2";
    if (region !== "us-west-2" && region !== "us-east-1" && region !== "eu-central-1" && region !== "ap-southeast-1") {
        throw invalidArgument("region must be us-west-2, us-east-1, eu-central-1, or ap-southeast-1");
    }
    const timeoutSeconds = options.timeoutSeconds ?? 300;
    integerRange(timeoutSeconds, 60, 21600, "timeoutSeconds");
    const domSettleTimeoutMs = options.domSettleTimeoutMs ?? 5000;
    integerRange(domSettleTimeoutMs, 0, 30000, "domSettleTimeoutMs");
    const contextId = options.contextId ?? "";
    if (contextId !== "") resourceId(contextId, "contextId");
    const persistContext = options.persistContext ?? false;
    if (persistContext && contextId === "") throw invalidArgument("persistContext requires contextId");
    const viewport = options.viewport;
    if (viewport !== undefined) {
        integerRange(viewport.width, 1, 8192, "viewport.width");
        integerRange(viewport.height, 1, 8192, "viewport.height");
    }
    return {
        model: model,
        selfHeal: options.selfHeal ?? true,
        domSettleTimeoutMs: domSettleTimeoutMs,
        region: region,
        timeoutSeconds: timeoutSeconds,
        keepAlive: options.keepAlive ?? false,
        proxies: options.proxies ?? false,
        verified: options.verified ?? false,
        solveCaptchas: options.solveCaptchas ?? false,
        contextId: contextId,
        persistContext: persistContext,
        viewport: viewport,
    };
}

/**
 * Build the session start body. Never includes projectId, a model API key, or a local CDP URL.
 * @param request Normalized start controls.
 * @returns JSON request body.
 */
export function buildStartBody(request: StagehandStart): string {
    const settings = [
        field("verified", JSON.stringify(request.verified)),
        field("solveCaptchas", JSON.stringify(request.solveCaptchas)),
    ];
    if (request.contextId !== "") {
        settings.push(field("context", objectJson([
            field("id", JSON.stringify(request.contextId)),
            field("persist", JSON.stringify(request.persistContext)),
        ])));
    }
    if (request.viewport !== undefined) {
        settings.push(field("viewport", objectJson([
            field("width", JSON.stringify(request.viewport.width)),
            field("height", JSON.stringify(request.viewport.height)),
        ])));
    }
    const createParams = [
        field("region", JSON.stringify(request.region)),
        field("timeout", JSON.stringify(request.timeoutSeconds)),
        field("keepAlive", JSON.stringify(request.keepAlive)),
        field("proxies", JSON.stringify(request.proxies)),
        field("browserSettings", objectJson(settings)),
    ];
    return objectJson([
        field("modelName", JSON.stringify(request.model)),
        field("selfHeal", JSON.stringify(request.selfHeal)),
        field("domSettleTimeoutMs", JSON.stringify(request.domSettleTimeoutMs)),
        field("browserbaseSessionCreateParams", objectJson(createParams)),
    ]);
}

/**
 * Read a start response and drop the CDP URL. The id must be safe to put in a path.
 * @param body Raw JSON body.
 * @returns The session id only.
 */
export function normalizeStartJson(body: string): string {
    const data = envelopeData(body);
    const sessionId = data.sessionId;
    if (typeof sessionId !== "string") throw invalidResponse();
    const lower = sessionId.toLowerCase();
    if (lower.includes("apikey=") || lower.includes("signingkey=") || lower.includes("apikey%3d") || lower.includes("signingkey%3d")) {
        throw invalidResponse();
    }
    try {
        return resourceId(sessionId, "sessionId");
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw invalidResponse();
        throw cause;
    }
}

/**
 * Build a navigate body. `waitUntil` defaults to `domcontentloaded`.
 * @param url Absolute URL already validated by the caller.
 * @param waitUntil Navigation condition.
 * @param timeoutMs Optional timeout.
 * @returns JSON request body.
 */
export function buildGotoBody(url: string, waitUntil: string, timeoutMs: number | undefined): string {
    if (!WAIT_UNTIL.includes(waitUntil)) throw invalidArgument("waitUntil must be load, domcontentloaded, or networkidle");
    const options = [field("waitUntil", JSON.stringify(waitUntil))];
    if (timeoutMs !== undefined) {
        integerRange(timeoutMs, 1, 120000, "timeoutMs");
        options.push(field("timeout", JSON.stringify(timeoutMs)));
    }
    return objectJson([field("url", JSON.stringify(url)), field("options", objectJson(options))]);
}

/**
 * Copy an observed action so a later read cannot change the replay.
 * @param action Caller action.
 * @returns A package-owned action.
 */
export function copyAction(action: ObservedAction): ObservedAction {
    const selector = action.selector;
    const description = action.description;
    const method = action.method;
    const backendNodeId = action.backendNodeId;
    if (selector.trim().length === 0 || selector.length > 2000) throw invalidArgument("action selector must be 1–2000 characters");
    if (description.trim().length === 0 || description.length > 2000) throw invalidArgument("action description must be 1–2000 characters");
    if (method !== null && (method.trim().length === 0 || method.length > 64)) throw invalidArgument("action method must be 1–64 characters or null");
    if (backendNodeId !== null && (!Number.isFinite(backendNodeId) || backendNodeId < 0)) {
        throw invalidArgument("action backendNodeId must be a non-negative number or null");
    }
    const args = action.arguments;
    const copied: string[] = [];
    if (args !== undefined && args !== null) {
        integerRange(args.length, 0, 20, "action arguments count");
        for (const argument of args) {
            if (argument.length > 4000) throw invalidArgument("action argument must be at most 4000 characters");
            copied.push(argument);
        }
    }
    return { selector: selector, description: description, method: method, arguments: copied, backendNodeId: backendNodeId };
}

/**
 * Build an act body from an instruction. Variables are included only when present.
 * @param instruction Atomic instruction, 1–2000 characters.
 * @param variables Optional placeholders, already copied.
 * @param timeoutMs Optional timeout.
 * @returns JSON request body.
 */
export function buildActBody(instruction: string, variables: NamedVariable[] | undefined, timeoutMs: number | undefined): string {
    requireLength(instruction, 1, 2000, "instruction");
    return objectJson([field("input", JSON.stringify(instruction)), field("options", actOptions(variables, timeoutMs))]);
}

/**
 * Build an act body that replays an observed action. No instruction, so no model inference.
 * @param action Package-owned action.
 * @param timeoutMs Optional timeout.
 * @returns JSON request body.
 */
export function buildReplayBody(action: ObservedAction, timeoutMs: number | undefined): string {
    const fields = [
        field("selector", JSON.stringify(action.selector)),
        field("description", JSON.stringify(action.description)),
    ];
    if (action.method !== null) fields.push(field("method", JSON.stringify(action.method)));
    if (action.arguments.length > 0) fields.push(field("arguments", JSON.stringify(action.arguments)));
    if (action.backendNodeId !== null) fields.push(field("backendNodeId", JSON.stringify(action.backendNodeId)));
    const options = timeoutMs === undefined ? "{}" : objectJson([field("timeout", JSON.stringify(checkedTimeout(timeoutMs)))]);
    return objectJson([field("input", objectJson(fields)), field("options", options)]);
}

/**
 * Build an observe body. An omitted instruction observes the page generally.
 * @param instruction Optional instruction.
 * @param selector Optional scope selector.
 * @param ignoreSelectors Optional exclusions, already copied.
 * @param timeoutMs Optional timeout.
 * @returns JSON request body.
 */
export function buildObserveBody(instruction: string | undefined, selector: string | undefined, ignoreSelectors: string[] | undefined, timeoutMs: number | undefined): string {
    const fields: string[] = [];
    if (instruction !== undefined) {
        requireLength(instruction, 1, 2000, "instruction");
        fields.push(field("instruction", JSON.stringify(instruction)));
    }
    const options = scopeOptions(selector, ignoreSelectors, timeoutMs, false);
    if (options !== undefined) fields.push(field("options", options));
    return objectJson(fields);
}

/**
 * Build an extract body.
 * @param instruction Optional instruction.
 * @param schema JSON schema fragment, already validated, or undefined.
 * @param selector Optional scope selector.
 * @param ignoreSelectors Optional exclusions, already copied.
 * @param screenshot Whether to attach a screenshot.
 * @param timeoutMs Optional timeout.
 * @returns JSON request body.
 */
export function buildExtractBody(instruction: string | undefined, schema: string | undefined, selector: string | undefined, ignoreSelectors: string[] | undefined, screenshot: boolean, timeoutMs: number | undefined): string {
    const fields: string[] = [];
    if (instruction !== undefined) {
        requireLength(instruction, 1, 2000, "instruction");
        fields.push(field("instruction", JSON.stringify(instruction)));
    }
    if (schema !== undefined) fields.push(field("schema", schemaObject(schema)));
    const options = scopeOptions(selector, ignoreSelectors, timeoutMs, screenshot);
    if (options !== undefined) fields.push(field("options", options));
    return objectJson(fields);
}

/**
 * Normalize an act response.
 * @param body Raw JSON body.
 * @returns The action result. A false `success` flag is returned, not thrown.
 */
export function normalizeActJson(body: string): ActResult {
    const data = envelopeData(body);
    const result = data.result;
    if (result === null || typeof result !== "object") throw invalidResponse();
    const record = result as Record<string, unknown>;
    if (typeof record.success !== "boolean" || typeof record.message !== "string" || typeof record.actionDescription !== "string") {
        throw invalidResponse();
    }
    return {
        success: record.success,
        message: record.message,
        actionDescription: record.actionDescription,
        actions: actionsFrom(record.actions),
        actionId: stringOrNull(data.actionId),
    };
}

/**
 * Normalize an observe response.
 * @param body Raw JSON body.
 * @returns Planned actions. Empty when nothing matched.
 */
export function normalizeObserveJson(body: string): ObservedAction[] {
    const data = envelopeData(body);
    return actionsFrom(data.result);
}

/**
 * Normalize an extract response.
 * @param body Raw JSON body.
 * @returns Extracted data and the action id.
 */
export function normalizeExtractJson(body: string): ExtractResult {
    const data = envelopeData(body);
    const result = data["result"];
    const actionId = data["actionId"];
    return { data: result === undefined ? null : result, actionId: stringOrNull(actionId) };
}

/**
 * Require a successful end envelope. End has no data payload.
 * A 200 with `success: false` is a failed close, not a finished one.
 * @param body Raw JSON body.
 */
export function normalizeEndJson(body: string): void {
    requireSuccess(body);
}

/**
 * Require a successful navigation envelope. A 200 with `success: false` did not open the URL.
 * @param body Raw JSON body.
 */
export function normalizeNavigateJson(body: string): void {
    requireSuccess(body);
}

function requireSuccess(body: string): void {
    try {
        const envelope = JSON.parse(body) as { success?: unknown };
        if (envelope.success !== true) throw invalidResponse();
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

/**
 * Validate a scope selector.
 * @param selector Optional selector.
 * @returns The same selector when it is in range.
 */
export function scopeSelector(selector: string | undefined): string | undefined {
    if (selector === undefined) return undefined;
    if (selector.trim() !== selector || selector.length === 0 || selector.length > 500) {
        throw invalidArgument("selector must be 1–500 characters with no surrounding whitespace");
    }
    return selector;
}

function schemaObject(schema: string): string {
    if (schema.trim() !== schema || !schema.startsWith("{")) {
        throw invalidArgument("schema must be a JSON object string");
    }
    let parsed: unknown;
    try {
        parsed = JSON.parse(schema);
    } catch (cause) {
        throw invalidArgument("schema must be a JSON object string");
    }
    if (parsed === null || typeof parsed !== "object" || Array.isArray(parsed)) {
        throw invalidArgument("schema must be a JSON object string");
    }
    const encoded = JSON.stringify(parsed);
    if (encoded === undefined || !encoded.startsWith("{")) {
        throw invalidArgument("schema must be a JSON object string");
    }
    return encoded;
}

function actOptions(variables: NamedVariable[] | undefined, timeoutMs: number | undefined): string {
    const fields: string[] = [];
    if (timeoutMs !== undefined) fields.push(field("timeout", JSON.stringify(checkedTimeout(timeoutMs))));
    if (variables !== undefined && variables.length > 0) fields.push(field("variables", variablesJson(variables)));
    return objectJson(fields);
}

function scopeOptions(selector: string | undefined, ignoreSelectors: string[] | undefined, timeoutMs: number | undefined, screenshot: boolean): string | undefined {
    const fields: string[] = [];
    if (selector !== undefined) fields.push(field("selector", JSON.stringify(selector)));
    if (ignoreSelectors !== undefined) fields.push(field("ignoreSelectors", JSON.stringify(ignoreSelectors)));
    if (timeoutMs !== undefined) fields.push(field("timeout", JSON.stringify(checkedTimeout(timeoutMs))));
    if (screenshot) fields.push(field("screenshot", "true"));
    if (fields.length === 0) return undefined;
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

function actionsFrom(value: unknown): ObservedAction[] {
    if (!Array.isArray(value)) throw invalidResponse();
    const actions: ObservedAction[] = [];
    for (const item of value) {
        if (item === null || typeof item !== "object") throw invalidResponse();
        const record = item as Record<string, unknown>;
        if (typeof record.selector !== "string" || typeof record.description !== "string") throw invalidResponse();
        const args: string[] = [];
        if (record.arguments !== undefined && record.arguments !== null) {
            if (!Array.isArray(record.arguments)) throw invalidResponse();
            for (const argument of record.arguments) {
                if (typeof argument !== "string") throw invalidResponse();
                args.push(argument);
            }
        }
        actions.push({
            selector: record.selector,
            description: record.description,
            method: typeof record.method === "string" ? record.method : null,
            arguments: args,
            backendNodeId: typeof record.backendNodeId === "number" ? record.backendNodeId : null,
        });
    }
    return actions;
}

function envelopeData(body: string): Record<string, unknown> {
    try {
        const envelope = JSON.parse(body) as { success?: unknown; data?: unknown };
        if (envelope.success !== true) throw invalidResponse();
        const data = envelope.data;
        if (data === null || data === undefined || typeof data !== "object") throw invalidResponse();
        return data as Record<string, unknown>;
    } catch (cause) {
        if (cause instanceof BrowserbaseError) throw cause;
        throw invalidResponse();
    }
}

function stringOrNull(value: unknown): string | null {
    if (value === undefined || value === null) return null;
    if (typeof value !== "string" || value.length === 0) throw invalidResponse();
    return value;
}

function checkedTimeout(timeoutMs: number): number {
    integerRange(timeoutMs, 1, 120000, "timeoutMs");
    return timeoutMs;
}

function integerRange(value: number, min: number, max: number, name: string): void {
    if (!Number.isFinite(value) || value !== Math.floor(value) || value < min || value > max) {
        throw invalidArgument(name + " must be an integer between " + min.toString() + " and " + max.toString());
    }
}

function requireLength(value: string, min: number, max: number, name: string): void {
    if (value.trim() !== value || value.trim().length < min || value.length > max) {
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
    return new BrowserbaseError("invalid_response", "Stagehand returned an invalid response");
}
