import { label } from "submilli:test";
import { BrowserbaseError } from "@submilli/browserbase";
import {
    browserbaseHttpError,
    buildContextBody,
    buildFetchBody,
    buildSearchBody,
    fetchHost,
    normalizeFetchJson,
    normalizeLiveViewJson,
    normalizeMessagesJson,
    normalizeSearchJson,
    normalizeSessionJson,
} from "../src/internal";
import {
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
    stagehandStart,
} from "../src/stagehand";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        if (cause instanceof BrowserbaseError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function main(): void {
    label("search sends an explicit provider and mode and rejects a project id");
    const search = buildSearchBody("browserbase", { numResults: 10, provider: "parallel", mode: "advanced" });
    assert(search.includes('"query":"browserbase"') && search.includes('"provider":"parallel"') && search.includes('"mode":"advanced"'), "default search shape");
    assert(!search.includes("projectId"), "search has no project id");
    expectCode("invalid_argument", () => { buildSearchBody(" ", { numResults: 10, provider: "parallel", mode: "advanced" }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 26, provider: "parallel", mode: "advanced" }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 1, provider: "parallel", mode: "deep" }); });
    expectCode("invalid_argument", () => { buildSearchBody("q", { numResults: 1, provider: "exa", mode: "advanced" }); });
    const deep = buildSearchBody("q", { numResults: 1, provider: "exa", mode: "deep" });
    assert(deep.includes('"provider":"exa"') && deep.includes('"mode":"deep"'), "exa deep is explicit");

    label("fetch host is normalized and credentials are rejected");
    assert(fetchHost("https://EXAMPLE.com./docs") === "example.com", "trailing dot is not part of the host");
    expectCode("invalid_argument", () => { fetchHost("https://user:secret@example.com/"); });
    expectCode("invalid_argument", () => { fetchHost("https:user:secret@example.com/"); });
    expectCode("invalid_argument", () => { fetchHost("https:/user:secret@example.com/"); });
    expectCode("invalid_argument", () => { fetchHost("https:\\\\user:secret@example.com/"); });
    expectCode("invalid_argument", () => { fetchHost("https:/\t/user:secret@example.com/"); });
    expectCode("invalid_argument", () => { fetchHost("\u0001https://user:secret@example.com/"); });
    assert(fetchHost("https://example.com/foo@bar") === "example.com", "an at sign in the path is not userinfo");
    assert(fetchHost("https://example.com/?q=@") === "example.com", "an at sign in the query is not userinfo");
    expectCode("invalid_argument", () => { fetchHost("file:///etc/passwd"); });
    expectCode("invalid_argument", () => { fetchHost("https://example.com/ "); });
    const fetchBody = buildFetchBody("https://example.com", {
        allowRedirects: false, allowInsecureSsl: false, proxies: false, format: "markdown", contextId: "",
    });
    assert(fetchBody.includes('"proxies":false') && fetchBody.includes('"format":"markdown"') && !fetchBody.includes("contextId"), "safe fetch defaults");
    expectCode("invalid_argument", () => { buildFetchBody("https://example.com", {
        allowRedirects: false, allowInsecureSsl: false, proxies: false, format: "markdown", contextId: "../admin",
    }); });

    label("connection secrets are dropped from session and live view payloads");
    const cleaned = normalizeSessionJson('{"id":"sess_1","createdAt":"t","updatedAt":"t","startedAt":"t","expiresAt":"t","status":"RUNNING","proxyBytes":0,"keepAlive":false,"region":"us-west-2","connectUrl":"wss://connect.browserbase.com?apiKey=bb_live_secret","signingKey":"secret","seleniumRemoteUrl":"https://connect.browserbase.com?apiKey=bb_live_secret"}');
    const encoded = JSON.stringify(cleaned);
    assert(cleaned.id === "sess_1" && !encoded.includes("apiKey") && !encoded.includes("signingKey") && !encoded.includes("connectUrl"), "session secrets dropped");
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/sessions/1?apiKey=bb_live_secret","debuggerFullscreenUrl":"https://www.browserbase.com/sessions/1","pages":[],"wsUrl":"wss://connect.browserbase.com?signingKey=abc"}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?signingKey=abc","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[],"wsUrl":"wss://connect.browserbase.com"}'); });
    const pageUrl = normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[{"id":"p1","url":"https://example.com/docs?apiKey=pk_test_demo","title":"Docs","faviconUrl":"https://example.com/favicon.ico","debuggerUrl":"https://www.browserbase.com/devtools/page","debuggerFullscreenUrl":"https://www.browserbase.com/devtools/page/full"}]}');
    assert(pageUrl.pages[0].url.includes("apiKey=pk_test_demo"), "a page URL is not a connection secret");
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?apiKey%3Dsecret","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[],"wsUrl":"wss://connect.browserbase.com"}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?api%4Bey=bb_live_secret","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[],"wsUrl":"wss://connect.browserbase.com"}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?signing%4Bey=abc","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[],"wsUrl":"wss://connect.browserbase.com"}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?api%4Bey=bb_live_secret#%","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[]}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?apiKey%3\tDsecret","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[]}'); });
    expectCode("invalid_response", () => { normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools?api\tKey=bb_live_secret","debuggerFullscreenUrl":"https://www.browserbase.com/devtools","pages":[]}'); });
    const live = normalizeLiveViewJson('{"debuggerUrl":"https://www.browserbase.com/devtools/inspector.html?wss=connect.browserbase.com/debug/sess_1","debuggerFullscreenUrl":"https://www.browserbase.com/devtools-fullscreen/inspector.html?wss=connect.browserbase.com/debug/sess_1","pages":[{"id":"p1","url":"https://example.com","title":"Example","faviconUrl":"https://example.com/favicon.ico","debuggerUrl":"https://www.browserbase.com/devtools/page","debuggerFullscreenUrl":"https://www.browserbase.com/devtools/page/full"}],"wsUrl":"wss://connect.browserbase.com/debug/sess_1"}');
    assert(!JSON.stringify(live).includes("wsUrl") && live.pages.length === 1, "websocket URL omitted");

    label("stagehand start uses Model Gateway and does not return a CDP URL");
    const start = stagehandStart({});
    const startBody = buildStartBody(start);
    assert(start.model === "google/gemini-2.5-flash" && start.solveCaptchas === false && start.selfHeal === true, "stagehand defaults");
    assert(startBody.includes('"modelName":"google/gemini-2.5-flash"') && !startBody.includes("projectId") && !startBody.includes("apiKey") && !startBody.includes("cdpUrl"), "start body");
    const started = normalizeStartJson('{"success":true,"data":{"sessionId":"c4dbf3a9-9a58-4b22-8a1c-9f20f9f9e123","cdpUrl":"wss://connect.browserbase.com/?signingKey=abc123","available":true}}');
    assert(started === "c4dbf3a9-9a58-4b22-8a1c-9f20f9f9e123", "CDP URL dropped");
    expectCode("invalid_argument", () => { stagehandStart({ model: "not a model" }); });
    expectCode("invalid_response", () => { normalizeStartJson('{"success":true,"data":{"sessionId":"wss://connect.browserbase.com?apiKey=bb_live_secret","available":true}}'); });
    expectCode("invalid_response", () => { normalizeStartJson('{"success":true,"data":{"sessionId":"..","available":true}}'); });
    expectCode("invalid_response", () => { normalizeStartJson('{"success":true,"data":{"sessionId":"../admin","available":true}}'); });

    label("goto, act, observe, and extract bodies stay atomic and omit provider keys");
    const gotoBody = buildGotoBody("https://example.com", "domcontentloaded", undefined);
    assert(gotoBody.includes('"waitUntil":"domcontentloaded"') && !gotoBody.includes("projectId"), "goto waits for DOM content");
    const actBody = buildActBody("click the Sign in button", undefined, undefined);
    assert(actBody.includes('"input":"click the Sign in button"') && !actBody.includes("apiKey"), "atomic act");
    const replay = buildReplayBody({ selector: "#login", description: "Sign in", method: "click", arguments: [], backendNodeId: null }, undefined);
    assert(replay.includes('"selector":"#login"') && replay.includes('"method":"click"'), "replay uses the observed selector");
    const observed = buildObserveBody("find the email input", undefined, undefined, undefined);
    assert(observed.includes('"instruction":"find the email input"'), "observe instruction");
    const extracted = buildExtractBody("extract the title", '{"type":"object"}', undefined, undefined, false, undefined);
    assert(extracted.includes('"schema":{"type":"object"}') && !extracted.includes("screenshot"), "extract schema without a screenshot");
    expectCode("invalid_argument", () => { buildExtractBody("title", '{"type":"object"},"options":{"model":{"apiKey":"sk-test"}}}', undefined, undefined, false, undefined); });
    expectCode("invalid_argument", () => { buildExtractBody("title", '{"type":"object"},"streamResponse":true}', undefined, undefined, false, undefined); });
    expectCode("invalid_argument", () => { buildExtractBody("title", "{", undefined, undefined, false, undefined); });
    expectCode("invalid_argument", () => { buildExtractBody("title", '["array"]', undefined, undefined, false, undefined); });
    expectCode("invalid_argument", () => { buildActBody("  ", undefined, undefined); });

    label("stagehand responses preserve action success and drop nothing required");
    const acted = normalizeActJson('{"success":true,"data":{"result":{"success":true,"message":"clicked","actionDescription":"Sign in","actions":[{"selector":"#login","description":"Sign in","method":"click","arguments":[]}]},"actionId":"act_1"}}');
    assert(acted.success && acted.actions[0].method === "click" && acted.actionId === "act_1", "act result");
    const planned = normalizeObserveJson('{"success":true,"data":{"result":[{"selector":"#email","description":"Email"}]}}');
    assert(planned.length === 1 && planned[0].method === null && planned[0].arguments.length === 0, "sparse observe action");
    const data = normalizeExtractJson('{"success":true,"data":{"result":{"title":"Example"},"actionId":"ex_1"}}');
    const title = data.data as { title: string };
    assert(title.title === "Example" && data.actionId === "ex_1", "extract data");
    normalizeEndJson('{"success":true}');
    normalizeNavigateJson('{"success":true,"data":{"url":"https://example.com"}}');
    expectCode("invalid_response", () => { normalizeEndJson('{"success":false}'); });
    expectCode("invalid_response", () => { normalizeNavigateJson('{"success":false,"data":{}}'); });
    expectCode("invalid_response", () => { normalizeActJson('{"success":false,"data":{}}'); });
    expectCode("invalid_argument", () => { copyAction({ selector: "", description: "x", method: null, arguments: [], backendNodeId: null }); });

    label("context create trims the name and never sends a project id");
    assert(buildContextBody(undefined) === "{}", "unnamed context");
    assert(buildContextBody("  Login  ") === '{"name":"Login"}', "trimmed context name");
    expectCode("invalid_argument", () => { buildContextBody("   "); });

    label("a pause question is kept and other tool inputs are dropped");
    const paused = normalizeMessagesJson('{"data":[{"id":"m1","createdAt":"t","message":{"role":"assistant","content":[{"type":"tool-call","toolName":"pause","toolCallId":"c1","input":{"message":"Enter the 2FA code","secret":"hidden"}},{"type":"tool-call","toolName":"click","input":{"selector":"#x"}}]}}],"nextSince":"m1"}');
    assert(paused.messages[0].text === "Enter the 2FA code" && !paused.messages[0].text.includes("hidden") && !paused.messages[0].text.includes("#x"), "pause question only");

    label("search and fetch responses keep source URLs and reject a non-text fetch");
    const found = normalizeSearchJson('{"requestId":"r1","query":"browserbase","results":[{"id":"1","url":"https://example.com","title":"Example","author":null}]}');
    assert(found.results[0].url === "https://example.com" && found.results[0].image === null, "search hit");
    const page = normalizeFetchJson('{"id":"f1","statusCode":200,"headers":{"content-type":"text/html"},"content":"# Example","contentType":"text/html","encoding":"utf-8"}');
    assert(page.content === "# Example" && page.headers[0].name === "content-type" && page.statusCode === 200, "fetch page");
    expectCode("invalid_response", () => { normalizeFetchJson('{"id":"f1","statusCode":200,"headers":{},"content":{"title":"no"},"contentType":"application/json","encoding":"utf-8"}'); });

    label("http errors name quota without calling the key invalid");
    const quota = browserbaseHttpError(402, null);
    assert(quota.code === "quota_exceeded" && quota.status === 402 && quota.message.includes("not an invalid API key"), "quota is not a bad key");
    assert(quota.message.includes("quota"), "quota message");
    const limited = browserbaseHttpError(429, "30");
    assert(limited.code === "rate_limited" && limited.retryAfter === "30", "retry after is preserved");
}
