// Files product feedback that a person gave an assistant, and confirmed, as one
// issue in a configured Linear team, for the product team to review.
//
// The team and the Linear credential come from secrets, never from arguments,
// so a program can file feedback but cannot choose where it goes, read the
// team's issues, or see the key. The key is read at call time from
// `FEEDBACK_LINEAR_API_KEY`; the team, a key such as "ENG" or a team UUID,
// from `FEEDBACK_LINEAR_TEAM`. The issue goes to the team's triage state when
// the team has one, otherwise to its first backlog state.

import { post, Response } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

const ENDPOINT = "https://api.linear.app/graphql";

/** What the feedback is about. */
export type FeedbackKind = "bug" | "idea" | "praise" | "question" | "other";

/** One piece of feedback, clarified with the person and confirmed by them. */
export interface Feedback {
    /** The person's own words, verbatim, as they first gave the feedback. Required. */
    original: string;
    /** The one-line summary the person confirmed; it becomes the issue title. Required. */
    summary: string;
    /** Must be true: the person read the summary back and confirmed it. */
    confirmed: boolean;
    /** What the feedback is about; defaults to "other". */
    kind?: FeedbackKind;
    /** What the person was trying to do. */
    goal?: string;
    /** What happened. */
    happened?: string;
    /** What they expected instead. */
    expected?: string;
    /** The steps they took, or the screen or page they were on. */
    steps?: string;
    /** How much it matters to them, in their words. */
    impact?: string;
    /** Any suggestion they made. */
    suggestion?: string;
    /** Later words from the person that add to or correct the original, verbatim. */
    additions?: string;
    /** The person, as the assistant knows them (for example the signed-in user it was told). */
    user?: string;
    /** The workspace or installation the person was using. */
    workspace?: string;
    /** The page or screen the person was on. */
    page?: string;
    /** The error text they hit, if any. */
    error?: string;
    /** Anything else about the situation: the task being done, the session. */
    context?: string;
    /** True for a test filing: the title starts with "[TEST]". */
    test?: boolean;
}

/** The issue a piece of feedback was filed as. */
export interface FiledFeedback {
    /** Issue UUID. */
    id: string;
    /** Human identifier, such as "ENG-123". */
    identifier: string;
    /** Issue URL. */
    url: string;
    /** Issue title. */
    title: string;
    /** The state the issue was filed in, such as "Triage" or "Backlog". */
    state: string;
}

/** An issue's title and Markdown description, built from feedback. */
export interface FeedbackIssue {
    /** Issue title. */
    title: string;
    /** Markdown description. */
    description: string;
}

const TITLE_LIMIT = 120;
const FIELD_LIMIT = 4000;
const ORIGINAL_LIMIT = 8000;
const KINDS = ["bug", "idea", "praise", "question", "other"];
// Credential prefixes a person may paste with an error message. A word that
// starts with one of these is replaced before anything is sent.
const CREDENTIAL_PREFIXES = ["lin_api_", "lin_oauth_", "sk-", "ghp_", "gho_", "ghs_", "ghu_", "github_pat_",
    "xoxb-", "xoxp-", "xoxa-", "xapp-", "AKIA", "ASIA", "AIza", "glpat-", "Bearer"];

/**
 * Replace each whitespace-separated word that starts with a known credential prefix
 * with "[redacted]". Spacing and line breaks are kept.
 * @param text Text that may contain a pasted credential.
 * @returns The text with credential-looking words replaced.
 */
export function redactCredentials(text: string): string {
    let out = "";
    let word = "";
    let redactNext = false;
    for (let i = 0; i <= text.length; i++) {
        const ch = i < text.length ? text.charAt(i) : " ";
        const space = ch === " " || ch === "\n" || ch === "\t" || ch === "\r";
        if (!space) {
            word += ch;
            continue;
        }
        if (word.length > 0) {
            let secret = redactNext;
            redactNext = false;
            if (word === "Bearer") {
                redactNext = true;
            } else {
                for (const prefix of CREDENTIAL_PREFIXES) {
                    if (word.startsWith(prefix) && word.length > prefix.length + 7) secret = true;
                }
            }
            out += secret ? "[redacted]" : word;
            word = "";
        }
        if (i < text.length) out += ch;
    }
    return out;
}

function clip(text: string, limit: number): string {
    if (text.length <= limit) return text;
    return text.slice(0, limit) + "… [cut at " + limit + " characters]";
}

function clean(text: string | undefined, limit: number): string | undefined {
    if (text === undefined) return undefined;
    const trimmed = text.trim();
    if (trimmed.length === 0) return undefined;
    return clip(redactCredentials(trimmed), limit);
}

// A fence longer than any backtick run in the text, so quoted words stay literal:
// no Markdown, links, or mentions take effect.
function fenced(text: string): string {
    let longest = 0;
    let run = 0;
    for (let i = 0; i < text.length; i++) {
        if (text.charAt(i) === "`") {
            run++;
            if (run > longest) longest = run;
        } else {
            run = 0;
        }
    }
    let fence = "```";
    while (fence.length <= longest) fence += "`";
    return fence + "text\n" + text + "\n" + fence;
}

function oneLine(text: string): string {
    let out = "";
    for (let i = 0; i < text.length; i++) {
        const ch = text.charAt(i);
        out += ch === "\n" || ch === "\r" || ch === "\t" ? " " : ch;
    }
    return out.trim();
}

function kindOf(feedback: Feedback): string {
    const kind = feedback.kind;
    if (kind === undefined) return "other";
    if (!KINDS.includes(kind)) throw new Error("Feedback kind must be one of " + KINDS.join(", "));
    return kind;
}

/**
 * Validate feedback and build the issue title and description, without network or secrets.
 * Throws when the person has not confirmed it, or when the original words or summary are empty.
 * @param feedback The clarified, confirmed feedback.
 * @returns The issue title and Markdown description.
 */
export function buildFeedbackIssue(feedback: Feedback): FeedbackIssue {
    if (feedback.confirmed !== true) {
        throw new Error("Feedback is filed only after the person confirms the summary");
    }
    const original = clean(feedback.original, ORIGINAL_LIMIT);
    const summary = clean(feedback.summary, TITLE_LIMIT);
    if (original === undefined) throw new Error("Feedback needs the person's original words");
    if (summary === undefined) throw new Error("Feedback needs a confirmed summary");
    const kind = kindOf(feedback);
    const prefix = (feedback.test === true ? "[TEST] " : "") + "Feedback (" + kind + "): ";
    const title = prefix + oneLine(summary);

    const lines: string[] = [];
    lines.push("Product feedback from a person, filed by an assistant after they confirmed this summary.");
    if (feedback.test === true) lines.push("", "**This is a test filing. Delete or archive it after review.**");
    lines.push("", "## Summary", "", fenced(summary));
    const sections: string[][] = [
        ["What they were trying to do", feedback.goal ?? ""],
        ["What happened", feedback.happened ?? ""],
        ["What they expected", feedback.expected ?? ""],
        ["Steps or screen", feedback.steps ?? ""],
        ["How much it matters to them", feedback.impact ?? ""],
        ["Their suggestion", feedback.suggestion ?? ""],
    ];
    lines.push("", "## Clarified");
    let any = false;
    for (const section of sections) {
        const value = clean(section[1], FIELD_LIMIT);
        if (value === undefined) continue;
        any = true;
        lines.push("", "**" + section[0] + "**", "", fenced(value));
    }
    if (!any) lines.push("", "_The person chose not to add details._");
    lines.push("", "## Their words (verbatim)", "", fenced(original));
    const additions = clean(feedback.additions, ORIGINAL_LIMIT);
    if (additions !== undefined) lines.push("", "**Later additions or corrections**", "", fenced(additions));

    lines.push("", "## Context", "");
    const facts: string[][] = [
        ["User (as told to the assistant, not verified)", feedback.user ?? ""],
        ["Workspace", feedback.workspace ?? ""],
        ["Page or screen", feedback.page ?? ""],
        ["Error", feedback.error ?? ""],
        ["Situation", feedback.context ?? ""],
    ];
    for (const fact of facts) {
        const value = clean(fact[1], FIELD_LIMIT);
        lines.push("**" + fact[0] + "**", "", value === undefined ? "_not given_" : fenced(value), "");
    }
    return { title: title, description: lines.join("\n").trim() };
}

/** A workflow state of the feedback team. */
export interface TeamState {
    /** State UUID. */
    id: string;
    /** State name, such as "Triage". */
    name: string;
    /** Category, such as "triage" or "backlog". */
    type: string;
    /** Order within its category. */
    position: number;
}

interface FeedbackTeam {
    id: string;
    key: string;
    states: { nodes: TeamState[] };
}

interface TeamsData {
    teams: { nodes: FeedbackTeam[] };
}

interface TeamData {
    team: FeedbackTeam | null;
}

/**
 * Whether a team setting is a UUID rather than a team key.
 * @param team The `FEEDBACK_LINEAR_TEAM` value.
 * @returns True for the 8-4-4-4-12 hexadecimal UUID form.
 */
export function isTeamId(team: string): boolean {
    if (team.length !== 36) return false;
    for (let i = 0; i < team.length; i++) {
        const ch = team.charAt(i).toLowerCase();
        const dash = i === 8 || i === 13 || i === 18 || i === 23;
        if (dash ? ch !== "-" : "0123456789abcdef".indexOf(ch) < 0) return false;
    }
    return true;
}

interface CreatedIssue {
    id: string;
    identifier: string;
    url: string;
    title: string;
    state: { name: string } | null;
}

interface IssueCreateData {
    issueCreate: { success: boolean; issue: CreatedIssue | null };
}

interface GraphQlError {
    message: string;
}

interface Envelope<T> {
    data?: T | null;
    errors?: GraphQlError[];
}

const TEAM_FIELDS = "id key states(first: 100) { nodes { id name type position } }";
const TEAM_BY_KEY_QUERY = "query FeedbackTeamByKey($key: String!) { teams(first: 2, filter: { key: { eqIgnoreCase: $key } }) { nodes { " + TEAM_FIELDS + " } } }";
const TEAM_BY_ID_QUERY = "query FeedbackTeamById($id: String!) { team(id: $id) { " + TEAM_FIELDS + " } }";
const CREATE_QUERY = "mutation FileFeedback($input: IssueCreateInput!) { issueCreate(input: $input) { success issue { id identifier url title state { name } } } }";

/**
 * Pick the state new feedback goes to: the team's triage state, else its first backlog state.
 * @param states The team's workflow states.
 * @returns The chosen state, or undefined when the team has neither.
 */
export function feedbackState(states: TeamState[]): TeamState | undefined {
    let backlog: TeamState | undefined = undefined;
    for (const state of states) {
        if (state.type === "triage") return state;
        if (state.type === "backlog" && (backlog === undefined || state.position < backlog.position)) backlog = state;
    }
    return backlog;
}

function secret(name: string): string {
    const value = secrets.get(name);
    if (value === undefined || value.trim().length === 0) {
        throw new Error("Feedback filing is not set up: the " + name + " secret is not bound");
    }
    return value.trim();
}

function graphql(query: string, variables: { [key: string]: unknown }): Response {
    const token = secret("FEEDBACK_LINEAR_API_KEY");
    const headers = new Map<string, string>();
    headers.set("Authorization", token.startsWith("lin_api_") || token.startsWith("Bearer ") ? token : "Bearer " + token);
    const response = post(ENDPOINT, { query: query, variables: variables }, headers);
    if (!response.ok) {
        // The body is not echoed: it is Linear's, and the person may see this message.
        throw new Error("Linear request failed: HTTP " + response.status + " " + response.statusText);
    }
    return response;
}

function errorsOf(errors: GraphQlError[] | undefined): string | undefined {
    if (errors === undefined || errors.length === 0) return undefined;
    const parts: string[] = [];
    for (const error of errors) parts.push(error.message);
    return "Linear GraphQL error: " + parts.join("; ");
}

function findTeam(name: string): FeedbackTeam {
    const missing = "The feedback team named by FEEDBACK_LINEAR_TEAM was not found";
    if (isTeamId(name)) {
        const byId = graphql(TEAM_BY_ID_QUERY, { id: name }).json<Envelope<TeamData>>();
        const idError = errorsOf(byId.errors);
        if (idError !== undefined) throw new Error(idError);
        const team = byId.data?.team;
        if (team === undefined || team === null) throw new Error(missing);
        return team;
    }
    const byKey = graphql(TEAM_BY_KEY_QUERY, { key: name }).json<Envelope<TeamsData>>();
    const keyError = errorsOf(byKey.errors);
    if (keyError !== undefined) throw new Error(keyError);
    const nodes = byKey.data?.teams.nodes ?? [];
    if (nodes.length !== 1) throw new Error(missing);
    return nodes[0];
}

/**
 * File one piece of confirmed feedback as an issue in the configured team's triage or backlog state.
 * The team comes from the `FEEDBACK_LINEAR_TEAM` secret and the key from `FEEDBACK_LINEAR_API_KEY`;
 * neither is an argument, and neither appears in the result.
 * @param feedback The clarified, confirmed feedback.
 * @returns The filed issue.
 * @capability linear.app/submitFeedback { kind: string, test: boolean }
 */
export function submitFeedback(feedback: Feedback): FiledFeedback {
    // Read every field once, so what was checked is what is filed.
    const copy: Feedback = {
        original: feedback.original, summary: feedback.summary, confirmed: feedback.confirmed,
        kind: feedback.kind, goal: feedback.goal, happened: feedback.happened, expected: feedback.expected,
        steps: feedback.steps, impact: feedback.impact, suggestion: feedback.suggestion,
        additions: feedback.additions, user: feedback.user, workspace: feedback.workspace,
        page: feedback.page, error: feedback.error, context: feedback.context, test: feedback.test,
    };
    const issue = buildFeedbackIssue(copy);
    const kind = kindOf(copy);
    const test = copy.test === true;
    check("linear.app/submitFeedback", { kind: kind, test: test });
    const teamName = secret("FEEDBACK_LINEAR_TEAM");
    const team = findTeam(teamName);
    const input: { [key: string]: unknown } = { teamId: team.id, title: issue.title, description: issue.description };
    const state = feedbackState(team.states.nodes);
    if (state !== undefined) input["stateId"] = state.id;
    const created = graphql(CREATE_QUERY, { input: input }).json<Envelope<IssueCreateData>>();
    const createError = errorsOf(created.errors);
    if (createError !== undefined) throw new Error(createError);
    const payload = created.data?.issueCreate;
    const filed = payload?.issue;
    if (payload === undefined || !payload.success || filed === undefined || filed === null) {
        throw new Error("Linear did not create the feedback issue");
    }
    return {
        id: filed.id,
        identifier: filed.identifier,
        url: filed.url,
        title: filed.title,
        state: filed.state?.name ?? (state?.name ?? ""),
    };
}
