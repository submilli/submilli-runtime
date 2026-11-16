import { report } from "submilli:cost";
import { post } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";
import * as typesafe from "@submilli/typesafe";

// These mirror @submilli/typesafe's declarations, which a program that imports only
// this Package cannot see; values from typesafe's shared codec are structurally the same.
/** Yes/no question; instructions accept JSON strings, objects, or arrays. */
export interface NoulQuestion {
    /** Wire discriminator. */
    type: "noul";
    /** The full proposition to judge, including relevant state paths. */
    instructions: unknown;
    /** Optional true/false descriptions. */
    criteria?: Map<string, unknown>;
}
/** Select exactly one known option. */
export interface ChoiceQuestion {
    /** Wire discriminator. */
    type: "choice";
    /** The complete selection question. */
    instructions: unknown;
    /** Option names mapped to JSON descriptions; a description may be null. */
    criteria: Map<string, unknown>;
}
/** Rate one dimension against independently meaningful descriptions. */
export interface ScoreQuestion {
    /** Wire discriminator. */
    type: "score";
    /** The complete rating question. */
    instructions: unknown;
    /** Ordered descriptions for 2–10 levels, starting at index zero. */
    criteria: unknown[];
}
/** One independent judgment in a batch. */
export type Question = NoulQuestion | ChoiceQuestion | ScoreQuestion;

/** Probability of yes, not intensity. There is no separate confidence. */
export interface NoulAnswer {
    /** Matches the question type. */
    type: "noul";
    /** Probability of yes, from zero to one. */
    noul: number;
}
/** Selected option with uncertainty preserved. */
export interface ChoiceAnswer {
    /** Matches the question type. */
    type: "choice";
    /** Highest-probability option key. */
    choice: string;
    /** Probability for every option. */
    probabilities: Map<string, number>;
    /** Distribution concentration, not overall workflow correctness. */
    confidence: number;
}
/** Score is the weighted mean of zero-based level indices, not a probability. */
export interface ScoreAnswer {
    /** Matches the question type. */
    type: "score";
    /** Fractional position between zero and the last level index. */
    score: number;
    /** String level indices mapped to their descriptions. */
    legend: Map<string, unknown>;
    /** Probability for every level index. */
    probabilities: Map<string, number>;
    /** Distribution concentration, from zero to one. */
    confidence: number;
}
/** Runtime-checked answer variants. */
export type Answer = NoulAnswer | ChoiceAnswer | ScoreAnswer;
/** Provider token accounting. */
export interface Usage {
    /** Input tokens consumed. */
    input_tokens: number;
    /** Output tokens produced. */
    output_tokens: number;
}
/** A batch result preserves IDs, uncertainty, and usage. */
export interface BatchResponse {
    /** Model version that actually answered. */
    model: string;
    /** Answers keyed by the caller's question IDs. */
    answers: Map<string, Answer>;
    /** Provider token accounting for the whole request. */
    usage: Usage;
}
/** Shared evidence and the independent questions to evaluate against it. */
export interface BatchRequest {
    /** Relevant text or JSON object/array. This call does not fetch missing evidence. */
    state: unknown;
    /** Independent questions over the same state, keyed by caller-chosen IDs. */
    questions: Map<string, Question>;
    /** `clef` (the default) or `clef-flash`. */
    model?: string;
}

/** Optional settings for a single-question call. */
export interface CallOptions {
    /** `clef` (the default, 27B) or `clef-flash` (9B, cheaper, shorter hosted context). */
    model?: string;
}

/** Local errors have status 0. HTTP errors preserve retry-after; no automatic retries. */
export class ClefError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;
    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "ClefError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Workers AI hosts each size under its own route; the body's `model` must match it. */
const ROUTES = new Map<string, string>([
    ["clef", "@cf/cloudflare/clef"],
    ["clef-flash", "@cf/cloudflare/clef-flash"],
]);
const DEFAULT_MODEL = "clef";
/** Ends in `/` so the build can derive the host filter from the constant prefix. */
const API_BASE = "https://api.cloudflare.com/";

/** Evaluate one yes/no judgment in one HTTP call; returns probability of yes.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Optional descriptions of the outcomes, keyed `"true"` and `"false"`; omit it (or pass `undefined`) to send none.
 * @param options Optional call settings (such as `model`); omit it to use `clef`.
 * @returns The probability of yes, from zero to one.
 * @capability cloudflare.com/clef {}
 */
export function noul(state: unknown, instructions: unknown, criteria?: Map<string, unknown>,
    options: CallOptions = {}): NoulAnswer {
    check("cloudflare.com/clef", {});
    const answer = evaluateSingle(state, noulQuestion(instructions, criteria), options.model);
    if (!typesafe.isNoulAnswer(answer)) throw invalidResponse();
    return answer;
}

/** Select one known option in one HTTP call, preserving probabilities and confidence.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Option names mapped to their JSON descriptions; 1-255 options.
 * @param options Optional call settings (such as `model`); omit it to use `clef`.
 * @returns The highest-probability option, the probability of every option, and the confidence.
 * @capability cloudflare.com/clef {}
 */
export function choice(state: unknown, instructions: unknown, criteria: Map<string, unknown>,
    options: CallOptions = {}): ChoiceAnswer {
    check("cloudflare.com/clef", {});
    const answer = evaluateSingle(state, choiceQuestion(instructions, criteria), options.model);
    if (!typesafe.isChoiceAnswer(answer)) throw invalidResponse();
    return answer;
}

/** Rate one dimension in one HTTP call, preserving probabilities, legend, and confidence.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Ordered descriptions of 2-10 levels; level indices start at zero.
 * @param options Optional call settings (such as `model`); omit it to use `clef`.
 * @returns The fractional score, the legend of level descriptions, per-level probabilities and the confidence.
 * @capability cloudflare.com/clef {}
 */
export function score(state: unknown, instructions: unknown, criteria: unknown[],
    options: CallOptions = {}): ScoreAnswer {
    check("cloudflare.com/clef", {});
    const answer = evaluateSingle(state, scoreQuestion(instructions, criteria), options.model);
    if (!typesafe.isScoreAnswer(answer)) throw invalidResponse();
    return answer;
}

/** Evaluate independent semantic judgments together; code owns subsequent actions.
 * @param request The shared state, the questions keyed by caller-chosen IDs, and an optional model (`clef` or `clef-flash`).
 * @returns The answering model, one answer per question ID, and token usage for the whole request.
 * @capability cloudflare.com/clef {}
 */
export function batch(request: BatchRequest): BatchResponse {
    check("cloudflare.com/clef", {});
    return evaluate(request);
}

/**
 * Build a yes/no question without HTTP. Optional criteria keys are true and false.
 * @param instructions The full proposition to judge, as a string or a JSON object/array.
 * @param criteria Optional descriptions keyed `"true"` and `"false"`; omit it to send none.
 * @returns A validated yes/no question for use in a `batch` request.
 */
export function noulQuestion(instructions: unknown, criteria?: Map<string, unknown>): NoulQuestion {
    try {
        return typesafe.noulQuestion(instructions, criteria);
    } catch (cause) {
        throw asClefError(cause);
    }
}

/**
 * Build a selection question without HTTP; include an other/no-match option when needed.
 * @param instructions The complete selection question, as a string or a JSON object/array.
 * @param criteria Option names mapped to their JSON descriptions; 1-255 options.
 * @returns A validated selection question for use in a `batch` request.
 */
export function choiceQuestion(instructions: unknown, criteria: Map<string, unknown>): ChoiceQuestion {
    try {
        return typesafe.choiceQuestion(instructions, criteria);
    } catch (cause) {
        throw asClefError(cause);
    }
}

/**
 * Build a rating question without HTTP using 2–10 concrete, independently meaningful levels.
 * @param instructions The complete rating question, as a string or a JSON object/array.
 * @param criteria Ordered descriptions of 2-10 levels; level indices start at zero.
 * @returns A validated rating question for use in a `batch` request.
 */
export function scoreQuestion(instructions: unknown, criteria: unknown[]): ScoreQuestion {
    try {
        return typesafe.scoreQuestion(instructions, criteria);
    } catch (cause) {
        throw asClefError(cause);
    }
}

/**
 * Narrow an already-decoded answer to NoulAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `NoulAnswer`.
 */
export function isNoulAnswer(answer: Answer | null | undefined): answer is NoulAnswer {
    return typesafe.isNoulAnswer(answer);
}

/**
 * Narrow an already-decoded answer to ChoiceAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `ChoiceAnswer`.
 */
export function isChoiceAnswer(answer: Answer | null | undefined): answer is ChoiceAnswer {
    return typesafe.isChoiceAnswer(answer);
}

/**
 * Narrow an already-decoded answer to ScoreAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `ScoreAnswer`.
 */
export function isScoreAnswer(answer: Answer | null | undefined): answer is ScoreAnswer {
    return typesafe.isScoreAnswer(answer);
}

function evaluateSingle(state: unknown, question: Question, model: string | undefined): Answer {
    const questions = new Map<string, Question>();
    questions.set("answer", question);
    const result = evaluate({ state: state, questions: questions, model: model });
    const answer = result.answers.get("answer");
    if (answer === undefined) throw invalidResponse();
    return answer;
}

function evaluate(request: BatchRequest): BatchResponse {
    const model = request.model ?? DEFAULT_MODEL;
    const body = encode(request, model);
    const route = ROUTES.get(model);
    if (route === undefined) throw new ClefError("invalid_argument", "model must be clef or clef-flash");
    const credentials = readCredentials();
    const path = "client/v4/accounts/" + credentials.accountId + "/ai/run/" + route;
    const response = post(API_BASE + path, body, credentials.headers);
    if (!response.ok) throw clefHttpError(response.status, response.headers.get("retry-after") ?? null);
    const decoded = decodeEnvelope(response.body, request.questions);
    reportCost(decoded);
    return decoded;
}

function encode(request: BatchRequest, model: string): string {
    try {
        return typesafe.encodeBatchRequest(request, model);
    } catch (cause) {
        throw asClefError(cause);
    }
}

/**
 * Unwrap Workers AI's `{ success, errors, messages, result }` envelope and decode the result.
 * @param body The raw JSON response body.
 * @param questions The questions that were sent, used to verify the answers.
 * @returns The decoded batch response.
 */
function decodeEnvelope(body: string, questions: Map<string, Question>): BatchResponse {
    try {
        const envelope = JSON.parse(body) as { success: boolean; result: unknown };
        if (envelope.success !== true) throw invalidResponse();
        return typesafe.decodeBatchResult(envelope.result, questions);
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Report usage so the run's cost includes the request. No amount is reported:
 * Workers AI bills in neurons after a daily free allowance, so a per-token price
 * would overstate the cost for accounts still within it.
 * Reporting is best effort; a refused report must not lose the paid-for answer.
 * @param response The decoded response, naming the model that answered.
 */
function reportCost(response: BatchResponse): void {
    try {
        report({
            usd: undefined,
            usage: { input_tokens: response.usage.input_tokens, output_tokens: response.usage.output_tokens },
            label: response.model,
        });
    } catch (error: unknown) {
        // The answer stands; the run's cost misses this request.
    }
}

interface Credentials {
    accountId: string;
    headers: Map<string, string>;
}

function readCredentials(): Credentials {
    const accountId = (secrets.get("CLOUDFLARE_ACCOUNT_ID") ?? "").trim().toLowerCase();
    const token = (secrets.get("CLOUDFLARE_API_TOKEN") ?? "").trim();
    if (accountId.length === 0 || token.length === 0) {
        throw new ClefError("missing_credentials",
            "Bind CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_API_TOKEN in the blueprint before using Clef");
    }
    // The ID goes into the URL path, and the generated capability filter checks only the host.
    if (!/^[0-9a-f]{32}$/.test(accountId)) {
        throw new ClefError("invalid_credentials", "CLOUDFLARE_ACCOUNT_ID must be a 32-character hexadecimal account ID");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + token);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    return { accountId: accountId, headers: headers };
}

/**
 * Map HTTP failures without exposing request state, provider bodies, or credentials.
 * @param status The HTTP status code of the failed response.
 * @param retryAfter The `retry-after` response header value, or `null` when absent.
 * @returns A `ClefError` carrying a code derived from the status.
 */
function clefHttpError(status: number, retryAfter: string | null): ClefError {
    let code = "http_error";
    if (status === 400 || status === 422) code = "invalid_request";
    else if (status === 401) code = "unauthorized";
    else if (status === 403) code = "forbidden";
    else if (status === 429) code = "rate_limited";
    return new ClefError(code, "Cloudflare Workers AI request failed with HTTP " + status.toString(), status, retryAfter);
}

/**
 * Validation is shared with `@submilli/typesafe`; callers of this Package see only `ClefError`.
 * @param cause The thrown value.
 * @returns A `ClefError` for a `TypeSafeError`, otherwise the original error.
 */
function asClefError(cause: unknown): Error {
    if (cause instanceof typesafe.TypeSafeError) return new ClefError(cause.code, cause.message, cause.status, cause.retryAfter);
    if (cause instanceof Error) return cause;
    return new Error(String(cause));
}

function invalidResponse(): ClefError {
    return new ClefError("invalid_response", "Cloudflare Workers AI returned an invalid or mismatched response");
}
