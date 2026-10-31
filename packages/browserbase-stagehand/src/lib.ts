// Hosted Stagehand browser. The API key is read from the BROWSERBASE_API_KEY
// secret. Generated code never sees it, and this package never accepts or sends
// a project id. Connection URLs and signing keys are not returned.

import { get, post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import * as kv from "submilli:session";
import { encodeComponent } from "submilli:url";
import {
    ActOptions,
    ActResult,
    ExtractOptions,
    ExtractResult,
    GotoOptions,
    ObserveOptions,
    ObservedAction,
    STAGEHAND_AP_SOUTHEAST,
    STAGEHAND_EU_CENTRAL,
    STAGEHAND_US_EAST,
    STAGEHAND_US_WEST,
    StagehandOptions,
    buildActBody,
    buildExtractBody,
    buildGotoBody,
    buildObserveBody,
    buildReplayBody,
    buildStartBody,
    copyAction,
    normalizeActJson,
    normalizeEndJson,
    normalizeNavigateJson,
    normalizeExtractJson,
    normalizeObserveJson,
    normalizeStartJson,
    scopeSelector,
    stagehandStart,
} from "./stagehand";
import {
    BASE,
    BrowserbaseError,
    LiveView,
    NamedVariable,
    Viewport,
    browserbaseHttpError,
    copyVariables,
    pageTarget,
    liveQuery,
    normalizeLiveViewJson,
    normalizeSessionJson,
    resourceId,
} from "./internal";

export {
    BrowserbaseError,
    Viewport,
    NamedVariable,
    LivePage,
    LiveView,
} from "./internal";

/** Cleanup arguments registered for a Stagehand browser this package created. */
export interface BrowserCleanup {
    /** Browser id. The finalizer key is this id. */
    id: string;
    /** Region that selects the Stagehand host. `end` on the wrong host does not release it. */
    region: string;
}

/** The Stagehand browser stored for the current runtime session. */
export interface CurrentStagehand {
    /** Stagehand session id. */
    sessionId: string;
    /** Region stored with the id so a later execution calls the same host. */
    region: string;
}
export {
    StagehandOptions,
    GotoOptions,
    WaitUntil,
    ObservedAction,
    ActOptions,
    ActResult,
    ObserveOptions,
    ExtractOptions,
    ExtractResult,
} from "./stagehand";

/**
 * One Stagehand cloud browser. Call `goto`, `act`, `observe`, `extract`, and `end`
 * on the instance. `current` reuses the browser stored for this runtime session.
 */
export class StagehandSession {
    /** Browser id. Also the Browserbase session id for live view. */
    readonly sessionId: string;
    /** Region that selects the Stagehand host. Default `us-west-2`. */
    readonly region: string;

    /**
     * Attach to an existing browser id. This does not start a browser.
     * Pass the region used to start it. The default host cannot drive another region.
     * @param sessionId Browser id from `open` or `current`.
     * @param region Region from `open` or `current`. Omitted means `us-west-2`.
     */
    constructor(sessionId: string, region: string = "us-west-2") {
        this.sessionId = sessionId;
        this.region = region;
    }

    /**
     * Start a browser and register cleanup. Prefer `current` when the next execution
     * should reuse it. Registration needs a session store. A catchable refusal ends
     * the browser and rethrows, so the caller does not receive a handle.
     * @param options Optional model, region, timeout, proxy, Verified, captcha, and context controls.
     * @returns The new browser. Connection URLs are not included.
     * @capability browserbase.com/stagehand.start { model: string, proxies: boolean, verified: boolean, solveCaptchas: boolean, keepAlive: boolean, region: string, timeoutSeconds: number, contextId: string, persistContext: boolean }
     */
    static open(options: StagehandOptions = {}): StagehandSession {
        const { model, selfHeal, domSettleTimeoutMs, region, timeoutSeconds, keepAlive, proxies, verified, solveCaptchas, contextId, persistContext, viewport } = options;
        let ownedViewport: Viewport | undefined = undefined;
        if (viewport !== undefined) {
            const width = viewport.width;
            const height = viewport.height;
            ownedViewport = { width: width, height: height };
        }
        const request = stagehandStart({
            model: model,
            selfHeal: selfHeal,
            domSettleTimeoutMs: domSettleTimeoutMs,
            region: region,
            timeoutSeconds: timeoutSeconds,
            keepAlive: keepAlive,
            proxies: proxies,
            verified: verified,
            solveCaptchas: solveCaptchas,
            contextId: contextId,
            persistContext: persistContext,
            viewport: ownedViewport,
        });
        check("browserbase.com/stagehand.start", {
            model: request.model,
            proxies: request.proxies,
            verified: request.verified,
            solveCaptchas: request.solveCaptchas,
            keepAlive: request.keepAlive,
            region: request.region,
            timeoutSeconds: request.timeoutSeconds,
            contextId: request.contextId,
            persistContext: request.persistContext,
        });
        const body = buildStartBody(request);
        const regionName = request.region;
        const sessionId = resourceId(normalizeStartJson(requireOk(stagehandPost(regionName, "sessions/start", body))), "sessionId");
        const cleanup: BrowserCleanup = { id: sessionId, region: regionName };
        try {
            kv.registerFinalizer(sessionId, cleanup);
        } catch (error: Error) {
            try { endStagehandRemote(sessionId, regionName); } catch (ignored: Error) { /* keep the registration error */ }
            throw error;
        }
        return new StagehandSession(sessionId, regionName);
    }

    /**
     * Return the browser stored for this runtime session, or start one and store it.
     * Needs `stagehand.start`, including when a stored browser is reused. A new browser
     * is started only when nothing reusable is stored. Closing the runtime session ends it.
     * @param options Options used only when a new browser is started. The start grant still sees them.
     * @returns The current browser.
     * @capability browserbase.com/stagehand.start { model: string, proxies: boolean, verified: boolean, solveCaptchas: boolean, keepAlive: boolean, region: string, timeoutSeconds: number, contextId: string, persistContext: boolean }
     */
    static current(options: StagehandOptions = {}): StagehandSession {
        const { model, selfHeal, domSettleTimeoutMs, region, timeoutSeconds, keepAlive, proxies, verified, solveCaptchas, contextId, persistContext, viewport } = options;
        let ownedViewport: Viewport | undefined = undefined;
        if (viewport !== undefined) {
            const width = viewport.width;
            const height = viewport.height;
            ownedViewport = { width: width, height: height };
        }
        const request = stagehandStart({
            model: model,
            selfHeal: selfHeal,
            domSettleTimeoutMs: domSettleTimeoutMs,
            region: region,
            timeoutSeconds: timeoutSeconds,
            keepAlive: keepAlive,
            proxies: proxies,
            verified: verified,
            solveCaptchas: solveCaptchas,
            contextId: contextId,
            persistContext: persistContext,
            viewport: ownedViewport,
        });
        check("browserbase.com/stagehand.start", {
            model: request.model,
            proxies: request.proxies,
            verified: request.verified,
            solveCaptchas: request.solveCaptchas,
            keepAlive: request.keepAlive,
            region: request.region,
            timeoutSeconds: request.timeoutSeconds,
            contextId: request.contextId,
            persistContext: request.persistContext,
        });
        const stored = kv.get<CurrentStagehand>("stagehand");
        if (stored !== undefined) {
            const sessionId = stored.sessionId;
            const regionName = stored.region;
            if (storedIdIsReusable(sessionId) && stagehandRegion(regionName) !== undefined) {
                if (stagehandAlive(sessionId)) return new StagehandSession(sessionId, regionName);
                const created = StagehandSession.open({
                    model: request.model,
                    selfHeal: request.selfHeal,
                    domSettleTimeoutMs: request.domSettleTimeoutMs,
                    region: request.region,
                    timeoutSeconds: request.timeoutSeconds,
                    keepAlive: request.keepAlive,
                    proxies: request.proxies,
                    verified: request.verified,
                    solveCaptchas: request.solveCaptchas,
                    contextId: request.contextId,
                    persistContext: request.persistContext,
                    viewport: ownedViewport,
                });
                kv.unregisterFinalizer(sessionId);
                kv.set("stagehand", { sessionId: created.sessionId, region: created.region });
                return created;
            }
            kv.remove("stagehand");
        }
        const created = StagehandSession.open({
            model: request.model,
            selfHeal: request.selfHeal,
            domSettleTimeoutMs: request.domSettleTimeoutMs,
            region: request.region,
            timeoutSeconds: request.timeoutSeconds,
            keepAlive: request.keepAlive,
            proxies: request.proxies,
            verified: request.verified,
            solveCaptchas: request.solveCaptchas,
            contextId: request.contextId,
            persistContext: request.persistContext,
            viewport: ownedViewport,
        });
        kv.set("stagehand", { sessionId: created.sessionId, region: created.region });
        return created;
    }

    /**
     * Open a URL. The check covers the submitted host, path, and protocol, not redirects.
     * @param url Absolute HTTP or HTTPS URL, without credentials.
     * @param options Optional wait condition and timeout. Omit it to wait for `domcontentloaded`.
     * @capability browserbase.com/navigate { host: string, path: string, protocol: string }
     */
    goto(url: string, options: GotoOptions = {}): void {
        const target = pageTarget(url);
        check("browserbase.com/navigate", {
            host: target.host,
            path: target.path,
            protocol: target.protocol,
        });
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const { waitUntil, timeoutMs } = options;
        const settle = waitUntil ?? "domcontentloaded";
        const body = buildGotoBody(url, settle, timeoutMs);
        normalizeNavigateJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/navigate", body)));
    }

    /**
     * Perform one atomic action, such as "click the Sign in button".
     * A false `success` flag is returned, not thrown. Use `replay` for an observed selector.
     * @param instruction One action, 1–2000 characters.
     * @param options Optional placeholders and timeout.
     * @returns What Stagehand did.
     * @capability browserbase.com/stagehand {}
     */
    act(instruction: string, options: ActOptions = {}): ActResult {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const { variables, timeoutMs } = options;
        let ownedVariables: NamedVariable[] | undefined = undefined;
        if (variables !== undefined) {
            const copied: NamedVariable[] = [];
            for (const variable of variables) {
                const name = variable.name;
                const value = variable.value;
                const description = variable.description;
                const entry: NamedVariable = { name: name, value: value };
                if (description !== undefined) entry.description = description;
                copied.push(entry);
            }
            ownedVariables = copyVariables(copied);
        }
        const body = buildActBody(instruction, ownedVariables, timeoutMs);
        return normalizeActJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/act", body)));
    }

    /**
     * Replay one action returned by `observe`. This does not call the model.
     * @param action Action previously returned by `observe`.
     * @param timeoutMs Optional timeout in milliseconds, 1–120000.
     * @returns What Stagehand did.
     * @capability browserbase.com/stagehand {}
     */
    replay(action: ObservedAction, timeoutMs?: number): ActResult {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const selector = action.selector;
        const description = action.description;
        const method = action.method;
        const backendNodeId = action.backendNodeId;
        const args = action.arguments;
        const copiedArgs: string[] = [];
        for (const argument of args) copiedArgs.push(argument);
        const owned = copyAction({ selector: selector, description: description, method: method, arguments: copiedArgs, backendNodeId: backendNodeId });
        const body = buildReplayBody(owned, timeoutMs);
        return normalizeActJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/act", body)));
    }

    /**
     * Plan actions on the current page without performing them.
     * @param instruction What to look for. Omit it to observe the page generally.
     * @param options Optional scope selectors and timeout.
     * @returns Planned actions. Empty when nothing matched.
     * @capability browserbase.com/stagehand {}
     */
    observe(instruction?: string, options: ObserveOptions = {}): ObservedAction[] {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const { selector, ignoreSelectors, timeoutMs } = options;
        const scope = scopeSelector(selector);
        let ignored: string[] | undefined = undefined;
        if (ignoreSelectors !== undefined) {
            ignored = [];
            for (const item of ignoreSelectors) ignored.push(item);
        }
        const body = buildObserveBody(instruction, scope, ignored, timeoutMs);
        return normalizeObserveJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/observe", body)));
    }

    /**
     * Extract structured data from the current page. Does not click or navigate.
     * @param instruction What to extract. Omit it to extract against `schema` alone.
     * @param schema JSON Schema text. Pass `JSON.stringify(schema)` so this method does not re-read a caller object. Omit it for an untyped result.
     * @param options Optional scope selectors, screenshot, and timeout.
     * @returns Extracted data and the provider action id.
     * @capability browserbase.com/stagehand {}
     */
    extract(instruction?: string, schema?: string, options: ExtractOptions = {}): ExtractResult {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const { selector, ignoreSelectors, screenshot, timeoutMs } = options;
        const scope = scopeSelector(selector);
        let ignored: string[] | undefined = undefined;
        if (ignoreSelectors !== undefined) {
            ignored = [];
            for (const item of ignoreSelectors) ignored.push(item);
        }
        const body = buildExtractBody(instruction, schema, scope, ignored, screenshot ?? false, timeoutMs);
        return normalizeExtractJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/extract", body)));
    }

    /**
     * End this browser. A missing browser is not an error.
     * @capability browserbase.com/stagehand {}
     */
    end(): void {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        endStagehandRemote(sessionId, regionName);
        kv.unregisterFinalizer(sessionId);
        forgetCurrent(sessionId);
    }

    /**
     * Live view links for this browser. Open `debuggerFullscreenUrl` to watch it.
     * @param expiresIn Optional URL lifetime in seconds, 60–21600.
     * @returns Debugger links and open pages. The CDP websocket URL is not returned.
     * @capability browserbase.com/stagehand {}
     */
    liveView(expiresIn?: number): LiveView {
        check("browserbase.com/stagehand", {});
        const sessionId = resourceId(this.sessionId, "sessionId");
        const query = expiresIn === undefined ? "" : liveQuery(expiresIn);
        return normalizeLiveViewJson(requireOk(get(BASE + "/v1/sessions/" + encodeComponent(sessionId) + "/debug" + query, authHeaders())));
    }
}

/**
 * Release a browser this package created. Called by the runtime when the session closes.
 * Remote close is idempotent. This does not register or unregister finalizers.
 * @param key Local finalizer key, the browser id.
 * @param value Cleanup arguments stored at registration.
 */
export function finalizer(key: string, value: BrowserCleanup): void {
    const id = value.id;
    const regionName = value.region;
    if (id !== key) throw new BrowserbaseError("invalid_argument", "cleanup key does not match the browser id");
    endStagehandRemote(id, regionName);
}

function stagehandAlive(sessionId: string): boolean {
    const id = resourceId(sessionId, "sessionId");
    const response = get(BASE + "/v1/sessions/" + encodeComponent(id), authHeaders());
    if (response.status === 404) return false;
    if (!response.ok) throw browserbaseHttpError(response.status, response.headers.get("retry-after") ?? null);
    const found = normalizeSessionJson(response.body);
    if (found.id !== id) throw new BrowserbaseError("invalid_response", "Browserbase returned a different session");
    return found.status === "RUNNING" || found.status === "PENDING";
}

function storedIdIsReusable(sessionId: string): boolean {
    try {
        resourceId(sessionId, "sessionId");
        return true;
    } catch (cause) {
        if (cause instanceof BrowserbaseError) return false;
        throw cause;
    }
}

function stagehandRegion(region: string): string | undefined {
    if (region === "us-west-2" || region === "us-east-1" || region === "eu-central-1" || region === "ap-southeast-1") return region;
    return undefined;
}

function stagehandPost(region: string, suffix: string, body: string | null): Response {
    const headers = authHeaders();
    if (region === "us-west-2") return post(STAGEHAND_US_WEST + suffix, body, headers);
    if (region === "us-east-1") return post(STAGEHAND_US_EAST + suffix, body, headers);
    if (region === "eu-central-1") return post(STAGEHAND_EU_CENTRAL + suffix, body, headers);
    if (region === "ap-southeast-1") return post(STAGEHAND_AP_SOUTHEAST + suffix, body, headers);
    throw new BrowserbaseError("invalid_argument", "region must be us-west-2, us-east-1, eu-central-1, or ap-southeast-1");
}

function endStagehandRemote(sessionId: string, region: string): void {
    const id = resourceId(sessionId, "sessionId");
    const response = stagehandPost(region, "sessions/" + encodeComponent(id) + "/end", null);
    // 404 means the browser is already gone. 409 is a conflict, not a finished close;
    // treating it as done would drop the finalizer while the browser can still be running.
    if (response.status === 404) return;
    if (!response.ok) throw browserbaseHttpError(response.status, response.headers.get("retry-after") ?? null);
    normalizeEndJson(response.body);
}

function forgetCurrent(sessionId: string): void {
    const stored = kv.get<CurrentStagehand>("stagehand");
    const storedId = stored === undefined ? undefined : stored.sessionId;
    if (storedId === sessionId) kv.remove("stagehand");
}

function snapshotSchema(schema: unknown): unknown {
    if (schema === undefined) return undefined;
    return JSON.parse(schemaFragment(schema));
}

function schemaFragment(schema: unknown): string {
    const encoded = JSON.stringify(schema);
    if (encoded === undefined || !encoded.startsWith("{")) {
        throw new BrowserbaseError("invalid_argument", "schema must be a JSON object");
    }
    return encoded;
}

function authHeaders(): Map<string, string> {
    const key = secrets.get("BROWSERBASE_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        throw new BrowserbaseError("missing_credentials", "Bind BROWSERBASE_API_KEY in the blueprint before using Browserbase");
    }
    const headers = new Map<string, string>();
    headers.set("X-BB-API-Key", key);
    headers.set("Accept", "application/json");
    headers.set("Content-Type", "application/json");
    return headers;
}

function requireOk(response: Response): string {
    if (!response.ok) throw browserbaseHttpError(response.status, response.headers.get("retry-after") ?? null);
    return response.body;
}
