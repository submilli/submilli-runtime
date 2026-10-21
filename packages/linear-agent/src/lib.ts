import { check } from "submilli:security";
import { parse } from "submilli:url";
import {
    createAgentActivity, getAgentSession, listAgentActivities, updateAgentSession,
    AgentActivityCreateInput, AgentActivityContent, AgentPromptContent, AgentSignalMetadata,
    AgentSession, AgentSessionExternalUrl, Page, PageOptions as LinearPageOptions,
} from "@submilli/linear";

/** Session snapshot; Linear derives its status from activities. */
export type Session = AgentSession;
/** A labeled link displayed on the session. */
export type ExternalLink = AgentSessionExternalUrl;
/** Explicit forward pagination; default first is 50, maximum 250. */
export type PageOptions = LinearPageOptions;
/** Recorded content includes incoming user prompts. */
export type ActivityContent = AgentActivityContent | AgentPromptContent;
/** Known modifiers, including incoming user stop/continue signals. */
export type ActivitySignal = "auth" | "select" | "stop" | "continue";
/** Typed metadata for recorded authentication and selection signals. */
export type SignalMetadata = AgentSignalMetadata;

/** A recorded activity, including its stable identity. */
export interface Activity {
    /** Activity UUID. */
    id: string;
    /** Creation timestamp. */
    createdAt: string;
    /** Whether the next agent activity replaces this activity. */
    ephemeral: boolean;
    /** Semantic activity content; prompt content is read-only. */
    content: ActivityContent;
    /** Known intent modifier, or null. */
    signal: ActivitySignal | null;
    /** Signal details, or null. */
    signalMetadata: SignalMetadata | null;
}
/** One explicitly requested page of activity history. */
export type ActivityPage = Page<Activity>;

/** Options for public progress updates. */
export interface ThoughtOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
    /** Replace this progress update when the next agent activity arrives. */
    ephemeral?: boolean;
}

/** Options for actions and their results. */
export interface ActionOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
    /** Replace this action when the next agent activity arrives. */
    ephemeral?: boolean;
    /** Markdown result when the action has completed. */
    result?: string;
}

/** Options for plain questions. */
export interface AskOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
}

/** Options for completed answers. */
export interface RespondOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
}

/** Options for published errors. */
export interface FailOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
}

/** Options for labeled selection requests. */
export interface AskChoiceOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
}

/** Options for account-linking requests. */
export interface AskAccountConnectionOptions {
    /** Optional caller-provided UUIDv4; no retry guarantee. */
    id?: string;
    /** Provider displayed in the account-linking prompt. */
    providerName?: string;
    /** Linear user allowed to complete account linking. */
    userId?: string;
}

/** One labeled selection; the user's reply may still be free text. */
export interface Choice {
    /** Label displayed to the user. */
    label: string;
    /** Value associated with this choice. */
    value: string;
}

/** Publish a short, user-visible progress update.
 * @param sessionId Existing session UUID.
 * @param body Markdown message.
 * @param options Optional activity UUID; ephemeral progress.
 * @returns The created activity, including its UUID.
 * @capability linear-agent.thought { sessionId: string }
 */
export function thought(sessionId: string, body: string, options?: ThoughtOptions): Activity {
    check("linear-agent.thought", { sessionId: sessionId });
    const input: AgentActivityCreateInput = { agentSessionId: sessionId, content: { type: "thought", body: body } };
    if (options?.id !== undefined) input.id = options.id;
    if (options?.ephemeral !== undefined) input.ephemeral = options.ephemeral;
    return createAgentActivity(input) as Activity;
}

/** Publish an action and, optionally, its completed result.
 * @param sessionId Existing session UUID.
 * @param action Action description, such as Searching.
 * @param parameter Action input, such as Related issues.
 * @param options Optional UUID, ephemeral flag, and Markdown result.
 * @returns The created activity, including its UUID.
 * @capability linear-agent.action { sessionId: string }
 */
export function action(sessionId: string, action: string, parameter: string, options?: ActionOptions): Activity {
    check("linear-agent.action", { sessionId: sessionId });
    const content: AgentActivityContent = { type: "action", action: action, parameter: parameter };
    if (options?.result !== undefined) content.result = options.result;
    const input: AgentActivityCreateInput = { agentSessionId: sessionId, content: content };
    if (options?.id !== undefined) input.id = options.id;
    if (options?.ephemeral !== undefined) input.ephemeral = options.ephemeral;
    return createAgentActivity(input) as Activity;
}

/** Ask a question; Linear waits for user input, but local execution continues.
 * @param sessionId Existing session UUID.
 * @param body Markdown message.
 * @param options Optional activity UUID.
 * @returns The created activity, including its UUID.
 * @capability linear-agent.ask { sessionId: string }
 */
export function ask(sessionId: string, body: string, options?: AskOptions): Activity {
    check("linear-agent.ask", { sessionId: sessionId });
    const input: AgentActivityCreateInput = { agentSessionId: sessionId, content: { type: "elicitation", body: body } };
    if (options?.id !== undefined) input.id = options.id;
    return createAgentActivity(input) as Activity;
}

/** Publish a completed answer; local execution is controlled by the caller.
 * @param sessionId Existing session UUID.
 * @param body Markdown message.
 * @param options Optional activity UUID.
 * @returns The created activity, including its UUID.
 * @capability linear-agent.respond { sessionId: string }
 */
export function respond(sessionId: string, body: string, options?: RespondOptions): Activity {
    check("linear-agent.respond", { sessionId: sessionId });
    const input: AgentActivityCreateInput = { agentSessionId: sessionId, content: { type: "response", body: body } };
    if (options?.id !== undefined) input.id = options.id;
    return createAgentActivity(input) as Activity;
}

/** Publish a failure as an error activity.
 * @param sessionId Existing session UUID.
 * @param body Markdown message.
 * @param options Optional activity UUID.
 * @returns The created activity, including its UUID.
 * @capability linear-agent.fail { sessionId: string }
 */
export function fail(sessionId: string, body: string, options?: FailOptions): Activity {
    check("linear-agent.fail", { sessionId: sessionId });
    const input: AgentActivityCreateInput = { agentSessionId: sessionId, content: { type: "error", body: body } };
    if (options?.id !== undefined) input.id = options.id;
    return createAgentActivity(input) as Activity;
}

/** Ask the user to select a labeled choice; free-text replies remain possible.
 * @param sessionId Existing session UUID.
 * @param body Markdown question.
 * @param choices Labeled values to present.
 * @param options Optional activity UUID.
 * @returns The created elicitation activity.
 * @capability linear-agent.askChoice { sessionId: string }
 */
export function askChoice(sessionId: string, body: string, choices: Choice[], options?: AskChoiceOptions): Activity {
    check("linear-agent.askChoice", { sessionId: sessionId });
    const input: AgentActivityCreateInput = {
        agentSessionId: sessionId,
        content: { type: "elicitation", body: body },
        signal: "select",
        signalMetadata: { options: choices },
    };
    if (options?.id !== undefined) input.id = options.id;
    return createAgentActivity(input) as Activity;
}

/** Ask the user to connect an account over HTTPS; does not suspend local execution.
 * @param sessionId Existing session UUID.
 * @param body Markdown account-linking request.
 * @param url HTTPS URL with a nonempty host.
 * @param options Optional UUID, provider name, and target Linear user.
 * @returns The created authentication elicitation.
 * @capability linear-agent.askAccountConnection { sessionId: string }
 */
export function askAccountConnection(sessionId: string, body: string, url: string, options?: AskAccountConnectionOptions): Activity {
    check("linear-agent.askAccountConnection", { sessionId: sessionId });
    const parsed = parse(url);
    if (parsed.protocol !== "https" || parsed.host === "") {
        throw new Error("Account connection URL must use HTTPS with a nonempty host");
    }
    const metadata: AgentSignalMetadata = { url: url };
    if (options?.providerName !== undefined) metadata.providerName = options.providerName;
    if (options?.userId !== undefined) metadata.userId = options.userId;
    const input: AgentActivityCreateInput = {
        agentSessionId: sessionId,
        content: { type: "elicitation", body: body },
        signal: "auth",
        signalMetadata: metadata,
    };
    if (options?.id !== undefined) input.id = options.id;
    return createAgentActivity(input) as Activity;
}

/** Read a session snapshot.
 * @param sessionId Existing session UUID.
 * @returns Current session state and links.
 * @capability linear-agent.getSession { sessionId: string }
 */
export function getSession(sessionId: string): Session {
    check("linear-agent.getSession", { sessionId: sessionId });
    return getAgentSession(sessionId);
}

/** Read one page of history, including user prompts and their signals.
 * @param sessionId Existing session UUID.
 * @param page Optional first/after pagination; omit for the first 50 activities.
 * @returns Activities and pageInfo; pass endCursor as after while hasNextPage is true.
 * @capability linear-agent.listActivities { sessionId: string }
 */
export function listActivities(sessionId: string, page?: PageOptions): ActivityPage {
    check("linear-agent.listActivities", { sessionId: sessionId });
    return listAgentActivities(sessionId, page) as ActivityPage;
}

/** Replace all session links; an empty array clears them.
 * @param sessionId Existing session UUID.
 * @param links Labeled links to replace the list with.
 * @returns The updated session.
 * @capability linear-agent.setExternalLinks { sessionId: string }
 */
export function setExternalLinks(sessionId: string, links: ExternalLink[]): Session {
    check("linear-agent.setExternalLinks", { sessionId: sessionId });
    return updateAgentSession(sessionId, { externalUrls: links });
}

/** Add session links without replacing the existing list.
 * @param sessionId Existing session UUID.
 * @param links Labeled links to add.
 * @returns The updated session.
 * @capability linear-agent.addExternalLinks { sessionId: string }
 */
export function addExternalLinks(sessionId: string, links: ExternalLink[]): Session {
    check("linear-agent.addExternalLinks", { sessionId: sessionId });
    return updateAgentSession(sessionId, { addedExternalUrls: links });
}

/** Remove specific URLs from the session links.
 * @param sessionId Existing session UUID.
 * @param urls URLs to remove.
 * @returns The updated session.
 * @capability linear-agent.removeExternalLinks { sessionId: string }
 */
export function removeExternalLinks(sessionId: string, urls: string[]): Session {
    check("linear-agent.removeExternalLinks", { sessionId: sessionId });
    return updateAgentSession(sessionId, { removedExternalUrls: urls });
}
