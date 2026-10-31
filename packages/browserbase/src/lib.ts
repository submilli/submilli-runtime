// Browserbase REST client: Search, Fetch, sessions, contexts, and browser-agent runs.
// The API key is read from the BROWSERBASE_API_KEY secret. Generated code never
// sees it, and this package never accepts or sends a project id — the key resolves
// the project. Session connect URLs and signing keys are not returned; they embed
// the key. Use sessionLiveView for a watch link.

import { get, post, delete as del, Response } from "submilli:http";
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
    Agent,
    AgentOptions,
    AgentPage,
    AgentRun,
    BrowserbaseError,
    Context,
    CreatedContext,
    FetchOptions,
    FetchedPage,
    LiveView,
    MessagePage,
    MessagePageOptions,
    NamedVariable,
    PageOptions,
    RunListOptions,
    RunOptions,
    RunPage,
    SearchOptions,
    SearchResponse,
    Session,
    Viewport,
    browserbaseHttpError,
    buildAgentBody,
    buildContextBody,
    buildFetchBody,
    buildResumeBody,
    buildRunBody,
    buildSearchBody,
    contextName,
    copyVariables,
    createdContextId,
    fetchHost,
    listQuery,
    liveQuery,
    messageQuery,
    normalizeAgentJson,
    normalizeAgentPageJson,
    normalizeContextJson,
    normalizeFetchJson,
    normalizeLiveViewJson,
    normalizeMessagesJson,
    normalizeRunJson,
    normalizeRunPageJson,
    normalizeSearchJson,
    normalizeSessionJson,
    normalizeSessionListJson,
    resourceId,
    runRequest,
    runStatus,
    sessionStatus,
} from "./internal";

export {
    BrowserbaseError,
    SearchOptions,
    SearchResult,
    SearchResponse,
    FetchFormat,
    FetchOptions,
    Header,
    FetchedPage,
    Viewport,
    Session,
    LivePage,
    LiveView,
    Context,
    CreatedContext,
    AgentOptions,
    Agent,
    PageOptions,
    AgentPage,
    NamedVariable,
    RunOptions,
    RunCause,
    AgentRun,
    RunListOptions,
    RunPage,
    MessagePageOptions,
    RunMessage,
    MessagePage,
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
 * Search the web. Does not start a browser. Provider and mode are sent explicitly;
 * the default is Parallel `advanced`.
 * @param query Search text, 1–200 characters.
 * @param options Optional result count, provider, and mode. Omit it for 10 Parallel advanced results.
 * @returns Hits in provider order. `results` is empty when nothing matched. There is no cursor.
 * @capability browserbase.com/search { numResults: number, provider: string, mode: string }
 */
export function search(query: string, options: SearchOptions = {}): SearchResponse {
    const { numResults, provider, mode } = options;
    const count = numResults ?? 10;
    const selectedProvider = provider ?? "parallel";
    const selectedMode = mode ?? (selectedProvider === "exa" ? "fast" : "advanced");
    const body = buildSearchBody(query, { numResults: count, provider: selectedProvider, mode: selectedMode });
    check("browserbase.com/search", { numResults: count, provider: selectedProvider, mode: selectedMode });
    return normalizeSearchJson(requireOk(post(BASE + "/v1/search", body, authHeaders())));
}

/**
 * Fetch one page without starting a browser. The host check covers the submitted
 * URL. Redirects are a provider instruction; grant `browserbase.com/delegatedFetch`
 * only when `allowRedirects` is true.
 * @param url Absolute HTTP or HTTPS URL, without credentials.
 * @param options Optional redirect, TLS, proxy, format, and context controls. Omit it for markdown with those flags off.
 * @returns The origin status and page text. A non-2xx origin status is still a successful fetch.
 * @capability browserbase.com/fetch { host: string, proxies: boolean, allowRedirects: boolean, allowInsecureSsl: boolean, contextId: string }
 * @capability browserbase.com/delegatedFetch {}
 */
export function fetchPage(url: string, options: FetchOptions = {}): FetchedPage {
    const { allowRedirects, allowInsecureSsl, proxies, format, contextId } = options;
    const host = fetchHost(url);
    const redirects = allowRedirects ?? false;
    const insecure = allowInsecureSsl ?? false;
    const useProxies = proxies ?? false;
    const bodyFormat = format ?? "markdown";
    const context = contextId ?? "";
    const body = buildFetchBody(url, {
        allowRedirects: redirects,
        allowInsecureSsl: insecure,
        proxies: useProxies,
        format: bodyFormat,
        contextId: context,
    });
    check("browserbase.com/fetch", {
        host: host,
        proxies: useProxies,
        allowRedirects: redirects,
        allowInsecureSsl: insecure,
        contextId: context,
    });
    if (redirects) check("browserbase.com/delegatedFetch", {});
    return normalizeFetchJson(requireOk(post(BASE + "/v1/fetch", body, authHeaders())));
}

/**
 * Read one session.
 * @param sessionId Session id.
 * @returns The session, or null when Browserbase returns 404.
 * @capability browserbase.com/sessions.read { sessionId: string }
 */
export function getSession(sessionId: string): Session | null {
    const id = resourceId(sessionId, "sessionId");
    check("browserbase.com/sessions.read", { sessionId: id });
    const response = get(BASE + "/v1/sessions/" + encodeComponent(id), authHeaders());
    if (response.status === 404) return null;
    return normalizeSessionJson(requireOk(response));
}

/**
 * List sessions. The API returns one array and documents no cursor; this does not page.
 * @param status Optional status filter.
 * @returns Sessions in provider order.
 * @capability browserbase.com/sessions.list {}
 */
export function listSessions(status?: string): Session[] {
    let query = "";
    if (status !== undefined) {
        const selected = sessionStatus(status);
        query = "?status=" + encodeComponent(selected);
    }
    check("browserbase.com/sessions.list", {});
    return normalizeSessionListJson(requireOk(get(BASE + "/v1/sessions" + query, authHeaders())));
}

/**
 * Ask Browserbase to end a session. Sending `REQUEST_RELEASE` avoids leaving a browser running.
 * @param sessionId Session id.
 * @returns The session after the release request.
 * @capability browserbase.com/sessions.release { sessionId: string }
 */
export function releaseSession(sessionId: string): Session {
    const id = resourceId(sessionId, "sessionId");
    check("browserbase.com/sessions.release", { sessionId: id });
    const response = post(BASE + "/v1/sessions/" + encodeComponent(id), "{\"status\":\"REQUEST_RELEASE\"}", authHeaders());
    const released = normalizeSessionJson(requireOk(response));
    kv.unregisterFinalizer(id);
    forgetCurrent(id);
    return released;
}

/**
 * Live view links for a session. Open `debuggerFullscreenUrl` to watch the cloud browser.
 * @param sessionId Session id.
 * @param expiresIn Optional URL lifetime in seconds, 60–21600. Omit it to expire with the session.
 * @returns Debugger links and open pages. The CDP websocket URL is not returned.
 * @capability browserbase.com/sessions.read { sessionId: string }
 */
export function sessionLiveView(sessionId: string, expiresIn?: number): LiveView {
    const id = resourceId(sessionId, "sessionId");
    const query = expiresIn === undefined ? "" : liveQuery(expiresIn);
    check("browserbase.com/sessions.read", { sessionId: id });
    return normalizeLiveViewJson(requireOk(get(BASE + "/v1/sessions/" + encodeComponent(id) + "/debug" + query, authHeaders())));
}

/**
 * Create a context that can reuse cookies across later sessions and fetches.
 * @param name Optional name, unique in the project ignoring case. Whitespace is trimmed.
 * @returns The new id and the trimmed name. Encryption keys are not returned.
 * @capability browserbase.com/contexts.create {}
 */
export function createContext(name?: string): CreatedContext {
    const storedName = contextName(name);
    const body = buildContextBody(name);
    check("browserbase.com/contexts.create", {});
    const id = createdContextId(requireOk(post(BASE + "/v1/contexts", body, authHeaders())));
    return { id: id, name: storedName };
}

/**
 * Read a context.
 * @param contextId Context id.
 * @returns The context, or null when Browserbase returns 404.
 * @capability browserbase.com/contexts.read { contextId: string }
 */
export function getContext(contextId: string): Context | null {
    const id = resourceId(contextId, "contextId");
    check("browserbase.com/contexts.read", { contextId: id });
    const response = get(BASE + "/v1/contexts/" + encodeComponent(id), authHeaders());
    if (response.status === 404) return null;
    return normalizeContextJson(requireOk(response));
}

/**
 * Delete a context. A missing context is not an error.
 * @param contextId Context id.
 * @capability browserbase.com/contexts.delete { contextId: string }
 */
export function deleteContext(contextId: string): void {
    const id = resourceId(contextId, "contextId");
    check("browserbase.com/contexts.delete", { contextId: id });
    const response = del(BASE + "/v1/contexts/" + encodeComponent(id), authHeaders());
    if (response.status === 204 || response.status === 404) return;
    if (!response.ok) throw browserbaseHttpError(response.status, response.headers.get("retry-after") ?? null);
}

/**
 * Create a reusable agent. Runs can also be ad-hoc; an agent is only needed to reuse a prompt.
 * @param name Display name, 1–255 characters.
 * @param options Optional system prompt and JSON Schema result schema.
 * @returns The created agent.
 * @capability browserbase.com/agents.create {}
 */
export function createAgent(name: string, options: AgentOptions = {}): Agent {
    const { systemPrompt, resultSchema } = options;
    const schema = resultSchema === undefined ? undefined : snapshotSchema(resultSchema);
    const body = buildAgentBody(name, { systemPrompt: systemPrompt, resultSchema: schema });
    check("browserbase.com/agents.create", {});
    return normalizeAgentJson(requireOk(post(BASE + "/v1/agents", body, authHeaders())));
}

/**
 * Read an agent.
 * @param agentId Agent id.
 * @returns The agent, or null when Browserbase returns 404.
 * @capability browserbase.com/agents.read { agentId: string }
 */
export function getAgent(agentId: string): Agent | null {
    const id = resourceId(agentId, "agentId");
    check("browserbase.com/agents.read", { agentId: id });
    const response = get(BASE + "/v1/agents/" + encodeComponent(id), authHeaders());
    if (response.status === 404) return null;
    return normalizeAgentJson(requireOk(response));
}

/**
 * List agents, one page at a time. Pass `nextCursor` back as `cursor`.
 * @param options Optional page size, cursor, and creation-time bounds.
 * @returns One page. `nextCursor` is null on the last page.
 * @capability browserbase.com/agents.list { limit: number }
 */
export function listAgents(options: PageOptions = {}): AgentPage {
    const { limit, cursor, startAt, endAt } = options;
    const pageSize = limit ?? 20;
    const query = listQuery({ limit: pageSize, cursor: cursor, startAt: startAt, endAt: endAt });
    check("browserbase.com/agents.list", { limit: pageSize });
    return normalizeAgentPageJson(requireOk(get(BASE + "/v1/agents" + query, authHeaders())));
}

/**
 * Delete an agent. Existing runs are unaffected. A missing agent is not an error.
 * @param agentId Agent id.
 * @capability browserbase.com/agents.delete { agentId: string }
 */
export function deleteAgent(agentId: string): void {
    const id = resourceId(agentId, "agentId");
    check("browserbase.com/agents.delete", { agentId: id });
    const response = del(BASE + "/v1/agents/" + encodeComponent(id), authHeaders());
    if (response.status === 204 || response.status === 404) return;
    if (!response.ok) throw browserbaseHttpError(response.status, response.headers.get("retry-after") ?? null);
}

/**
 * Start a browser agent on a task. The run uses web search and a cloud browser.
 * This does not poll. Agent runs can spend Model Gateway tokens. Proxies and Verified default off.
 * @param task What the agent should do, 1–8000 characters.
 * @param options Optional agent id, schema, pause condition, variables, proxies, Verified, and context.
 * @returns The run in `PENDING`. Poll `getRun` for the result.
 * @capability browserbase.com/runs.start { proxies: boolean, verified: boolean, contextId: string, persistContext: boolean, agentId: string }
 */
export function runAgent(task: string, options: RunOptions = {}): AgentRun {
    const { agentId, resultSchema, pauseWhen, variables, proxies, verified, contextId, persistContext } = options;
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
    const request = runRequest(task, {
        agentId: agentId,
        pauseWhen: pauseWhen,
        variables: ownedVariables,
        proxies: proxies,
        verified: verified,
        contextId: contextId,
        persistContext: persistContext,
    });
    check("browserbase.com/runs.start", {
        proxies: request.proxies,
        verified: request.verified,
        contextId: request.contextId,
        persistContext: request.persistContext,
        agentId: request.agentId,
    });
    const schema = snapshotSchema(resultSchema);
    const body = buildRunBody({
        task: request.task,
        agentId: request.agentId,
        proxies: request.proxies,
        verified: request.verified,
        contextId: request.contextId,
        persistContext: request.persistContext,
        pauseWhen: request.pauseWhen,
        variables: request.variables,
        resultSchema: schema,
    });
    return normalizeRunJson(requireOk(post(BASE + "/v1/agents/runs", body, authHeaders())));
}

/**
 * Read one run, including its result when the run has finished.
 * @param runId Run id.
 * @returns The run, or null when Browserbase returns 404.
 * @capability browserbase.com/runs.read { runId: string }
 */
export function getRun(runId: string): AgentRun | null {
    const id = resourceId(runId, "runId");
    check("browserbase.com/runs.read", { runId: id });
    const response = get(BASE + "/v1/agents/runs/" + encodeComponent(id), authHeaders());
    if (response.status === 404) return null;
    return normalizeRunJson(requireOk(response));
}

/**
 * List runs, one page at a time.
 * @param options Optional page size, cursor, status, agent, and creation-time bounds.
 * @returns One page. `nextCursor` is null on the last page.
 * @capability browserbase.com/runs.list { limit: number }
 */
export function listRuns(options: RunListOptions = {}): RunPage {
    const { limit, cursor, status, agentId, startAt, endAt } = options;
    const pageSize = limit ?? 20;
    const selectedStatus = status === undefined ? undefined : runStatus(status);
    const query = listQuery({
        limit: pageSize,
        cursor: cursor,
        status: selectedStatus,
        agentId: agentId,
        startAt: startAt,
        endAt: endAt,
    });
    check("browserbase.com/runs.list", { limit: pageSize });
    return normalizeRunPageJson(requireOk(get(BASE + "/v1/agents/runs" + query, authHeaders())));
}

/**
 * Ask a running agent to stop. Stopping a finished run returns a conflict.
 * @param runId Run id.
 * @returns The run after the stop was accepted. Poll `getRun` until status is `STOPPED`.
 * @capability browserbase.com/runs.stop { runId: string }
 */
export function stopRun(runId: string): AgentRun {
    const id = resourceId(runId, "runId");
    check("browserbase.com/runs.stop", { runId: id });
    const response = post(BASE + "/v1/agents/runs/" + encodeComponent(id) + "/stop", null, authHeaders());
    return normalizeRunJson(requireOk(response));
}

/**
 * Resume a paused run with the reply to its pause. Resuming a run that is not paused returns a conflict.
 * @param runId Run id.
 * @param task Reply text, 1–8000 characters.
 * @param variables Optional placeholders merged over the run's original variables.
 * @returns The run, now running again.
 * @capability browserbase.com/runs.resume { runId: string }
 */
export function resumeRun(runId: string, task: string, variables?: NamedVariable[]): AgentRun {
    const id = resourceId(runId, "runId");
    const owned = variables === undefined ? undefined : copyVariables(variables);
    const body = buildResumeBody(task, owned);
    check("browserbase.com/runs.resume", { runId: id });
    const response = post(BASE + "/v1/agents/runs/" + encodeComponent(id) + "/resume", body, authHeaders());
    return normalizeRunJson(requireOk(response));
}

/**
 * Read one page of run messages, oldest first. Tool-call inputs are omitted.
 * @param runId Run id.
 * @param options Optional page size and the last message id already seen.
 * @returns Messages and `nextSince`. Pass `nextSince` back as `since`. Null `nextSince` means the run has no messages yet.
 * @capability browserbase.com/runs.read { runId: string }
 */
export function listRunMessages(runId: string, options: MessagePageOptions = {}): MessagePage {
    const { limit, since } = options;
    const id = resourceId(runId, "runId");
    const pageSize = limit ?? 20;
    const query = messageQuery(pageSize, since);
    check("browserbase.com/runs.read", { runId: id });
    return normalizeMessagesJson(requireOk(get(BASE + "/v1/agents/runs/" + encodeComponent(id) + "/messages" + query, authHeaders())));
}

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
     * @param sessionId Browser id from `open`, `current`, or `listSessions`.
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
        const body = buildStartBody(request);
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
     * The first call needs `stagehand.start`. A later call needs `stagehand.current`
     * when the stored browser is still pending or running. Closing the runtime session ends it.
     * @param options Options used only when a new browser is started.
     * @returns The current browser.
     * @capability browserbase.com/stagehand.current { sessionId: string }
     */
    static current(options: StagehandOptions = {}): StagehandSession {
        const stored = kv.get<CurrentStagehand>("stagehand");
        if (stored !== undefined) {
            const sessionId = stored.sessionId;
            const regionName = stored.region;
            if (storedIdIsReusable(sessionId) && stagehandRegion(regionName) !== undefined) {
                check("browserbase.com/stagehand.current", { sessionId: sessionId });
                if (stagehandAlive(sessionId)) return new StagehandSession(sessionId, regionName);
                const created = StagehandSession.open(options);
                kv.unregisterFinalizer(sessionId);
                kv.set("stagehand", { sessionId: created.sessionId, region: created.region });
                return created;
            }
            kv.remove("stagehand");
        }
        const created = StagehandSession.open(options);
        kv.set("stagehand", { sessionId: created.sessionId, region: created.region });
        return created;
    }

    /**
     * Open a URL. The host check covers the submitted URL. Redirects are a separate grant.
     * @param url Absolute HTTP or HTTPS URL, without credentials.
     * @param options Optional wait condition and timeout. Omit it to wait for `domcontentloaded`.
     * @capability browserbase.com/stagehand.navigate { sessionId: string, host: string }
     * @capability browserbase.com/delegatedFetch {}
     */
    goto(url: string, options: GotoOptions = {}): void {
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        const { waitUntil, timeoutMs } = options;
        const host = fetchHost(url);
        const settle = waitUntil ?? "domcontentloaded";
        const body = buildGotoBody(url, settle, timeoutMs);
        check("browserbase.com/stagehand.navigate", { sessionId: sessionId, host: host });
        check("browserbase.com/delegatedFetch", {});
        normalizeNavigateJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/navigate", body)));
    }

    /**
     * Perform one atomic action, such as "click the Sign in button".
     * A false `success` flag is returned, not thrown. Use `replay` for an observed selector.
     * @param instruction One action, 1–2000 characters.
     * @param options Optional placeholders and timeout.
     * @returns What Stagehand did.
     * @capability browserbase.com/stagehand.act { sessionId: string }
     */
    act(instruction: string, options: ActOptions = {}): ActResult {
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
        check("browserbase.com/stagehand.act", { sessionId: sessionId });
        return normalizeActJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/act", body)));
    }

    /**
     * Replay one action returned by `observe`. This does not call the model.
     * @param action Action previously returned by `observe`.
     * @param timeoutMs Optional timeout in milliseconds, 1–120000.
     * @returns What Stagehand did.
     * @capability browserbase.com/stagehand.act { sessionId: string }
     */
    replay(action: ObservedAction, timeoutMs?: number): ActResult {
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
        check("browserbase.com/stagehand.act", { sessionId: sessionId });
        return normalizeActJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/act", body)));
    }

    /**
     * Plan actions on the current page without performing them.
     * @param instruction What to look for. Omit it to observe the page generally.
     * @param options Optional scope selectors and timeout.
     * @returns Planned actions. Empty when nothing matched.
     * @capability browserbase.com/stagehand.observe { sessionId: string }
     */
    observe(instruction?: string, options: ObserveOptions = {}): ObservedAction[] {
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
        check("browserbase.com/stagehand.observe", { sessionId: sessionId });
        return normalizeObserveJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/observe", body)));
    }

    /**
     * Extract structured data from the current page. Does not click or navigate.
     * @param instruction What to extract. Omit it to extract against `schema` alone.
     * @param schema JSON Schema text. Pass `JSON.stringify(schema)` so this method does not re-read a caller object. Omit it for an untyped result.
     * @param options Optional scope selectors, screenshot, and timeout.
     * @returns Extracted data and the provider action id.
     * @capability browserbase.com/stagehand.extract { sessionId: string }
     */
    extract(instruction?: string, schema?: string, options: ExtractOptions = {}): ExtractResult {
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
        check("browserbase.com/stagehand.extract", { sessionId: sessionId });
        return normalizeExtractJson(requireOk(stagehandPost(regionName, "sessions/" + encodeComponent(sessionId) + "/extract", body)));
    }

    /**
     * End this browser. A missing browser is not an error.
     * @capability browserbase.com/stagehand.end { sessionId: string }
     */
    end(): void {
        const sessionId = resourceId(this.sessionId, "sessionId");
        const regionName = this.region;
        check("browserbase.com/stagehand.end", { sessionId: sessionId });
        endStagehandRemote(sessionId, regionName);
        kv.unregisterFinalizer(sessionId);
        forgetCurrent(sessionId);
    }

    /**
     * Live view links for this browser. Open `debuggerFullscreenUrl` to watch it.
     * @param expiresIn Optional URL lifetime in seconds, 60–21600.
     * @returns Debugger links and open pages. The CDP websocket URL is not returned.
     * @capability browserbase.com/sessions.read { sessionId: string }
     */
    liveView(expiresIn?: number): LiveView {
        const sessionId = resourceId(this.sessionId, "sessionId");
        const query = expiresIn === undefined ? "" : liveQuery(expiresIn);
        check("browserbase.com/sessions.read", { sessionId: sessionId });
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
