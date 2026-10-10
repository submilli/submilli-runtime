import { post } from "submilli:http";
import secrets from "submilli:secrets";
import { check } from "submilli:security";

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
    /** Defaults to jev-latest. Pin a version when evaluating fixed thresholds. */
    model?: string;
}

/** Optional settings for a single-question call. */
export interface CallOptions {
    /** Defaults to jev-latest; pin a version when evaluating fixed thresholds. */
    model?: string;
}

/** Local errors have status 0. HTTP errors preserve retry-after; no automatic retries. */
export class TypeSafeError extends Error {
    code: string;
    status: number;
    retryAfter: string | null;
    constructor(code: string, message: string, status: number = 0, retryAfter: string | null = null) {
        super(message);
        this.name = "TypeSafeError";
        this.code = code;
        this.status = status;
        this.retryAfter = retryAfter;
    }
}

/** Evaluate one yes/no judgment in one HTTP call; returns probability of yes.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Optional descriptions of the outcomes, keyed `"true"` and `"false"`; omit it (or pass `undefined`) to send none.
 * @param options Optional call settings (such as `model`); omit it to use the default model `jev-latest`.
 * @returns The probability of yes, from zero to one.
 * @capability typesafe.ai/systemone {}
 */
export function noul(state: unknown, instructions: unknown, criteria?: Map<string, unknown>,
    options: CallOptions = {}): NoulAnswer {
    const model = options.model;
    check("typesafe.ai/systemone", {});
    const answer = evaluateSingle(state, noulQuestion(instructions, criteria), model);
    if (!isNoulAnswer(answer)) throw invalidResponse();
    return answer;
}

/** Select one known option in one HTTP call, preserving probabilities and confidence.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Option names mapped to their JSON descriptions; 1-255 options.
 * @param options Optional call settings (such as `model`); omit it to use the default model `jev-latest`.
 * @returns The highest-probability option, the probability of every option, and the confidence.
 * @capability typesafe.ai/systemone {}
 */
export function choice(state: unknown, instructions: unknown, criteria: Map<string, unknown>,
    options: CallOptions = {}): ChoiceAnswer {
    const model = options.model;
    check("typesafe.ai/systemone", {});
    const answer = evaluateSingle(state, choiceQuestion(instructions, criteria), model);
    if (!isChoiceAnswer(answer)) throw invalidResponse();
    return answer;
}

/** Rate one dimension in one HTTP call, preserving probabilities, legend, and confidence.
 * @param state Evidence for the judgment: text or a JSON object/array. The call does not fetch missing evidence.
 * @param instructions The full question, as a string or a JSON object/array; must not be blank.
 * @param criteria Ordered descriptions of 2-10 levels; level indices start at zero.
 * @param options Optional call settings (such as `model`); omit it to use the default model `jev-latest`.
 * @returns The fractional score, the legend of level descriptions, per-level probabilities and the confidence.
 * @capability typesafe.ai/systemone {}
 */
export function score(state: unknown, instructions: unknown, criteria: unknown[],
    options: CallOptions = {}): ScoreAnswer {
    const model = options.model;
    check("typesafe.ai/systemone", {});
    const answer = evaluateSingle(state, scoreQuestion(instructions, criteria), model);
    if (!isScoreAnswer(answer)) throw invalidResponse();
    return answer;
}

/**
 * Build a yes/no question without HTTP. Optional criteria keys are true and false.
 * @param instructions The full proposition to judge, as a string or a JSON object/array.
 * @param criteria Optional descriptions keyed `"true"` and `"false"`; omit it to send none.
 * @returns A validated yes/no question for use in a `batch` request.
 */
export function noulQuestion(instructions: unknown, criteria?: Map<string, unknown>): NoulQuestion {
    const question: NoulQuestion = { type: "noul", instructions: instructions };
    if (criteria !== undefined) question.criteria = criteria;
    validateQuestion(question);
    return question;
}

/**
 * Build a selection question without HTTP; include an other/no-match option when needed.
 * @param instructions The complete selection question, as a string or a JSON object/array.
 * @param criteria Option names mapped to their JSON descriptions; 1-255 options.
 * @returns A validated selection question for use in a `batch` request.
 */
export function choiceQuestion(instructions: unknown, criteria: Map<string, unknown>): ChoiceQuestion {
    const question: ChoiceQuestion = { type: "choice", instructions: instructions, criteria: criteria };
    validateQuestion(question);
    return question;
}

/**
 * Build a rating question without HTTP using 2–10 concrete, independently meaningful levels.
 * @param instructions The complete rating question, as a string or a JSON object/array.
 * @param criteria Ordered descriptions of 2-10 levels; level indices start at zero.
 * @returns A validated rating question for use in a `batch` request.
 */
export function scoreQuestion(instructions: unknown, criteria: unknown[]): ScoreQuestion {
    const question: ScoreQuestion = { type: "score", instructions: instructions, criteria: criteria };
    validateQuestion(question);
    return question;
}

/** Evaluate independent semantic judgments together; code owns subsequent actions.
 * @param request The shared state, the questions keyed by caller-chosen IDs, and an optional model.
 * @returns The answering model, one answer per question ID, and token usage for the whole request.
 * @capability typesafe.ai/systemone {}
 */
export function batch(request: BatchRequest): BatchResponse {
    check("typesafe.ai/systemone", {});
    return evaluate(request);
}

/**
 * Narrow an already-decoded answer to NoulAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `NoulAnswer`.
 */
export function isNoulAnswer(answer: Answer | null | undefined): answer is NoulAnswer {
    return answer !== null && answer !== undefined && answer.type === "noul";
}

/**
 * Narrow an already-decoded answer to ChoiceAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `ChoiceAnswer`.
 */
export function isChoiceAnswer(answer: Answer | null | undefined): answer is ChoiceAnswer {
    return answer !== null && answer !== undefined && answer.type === "choice";
}

/**
 * Narrow an already-decoded answer to ScoreAnswer; returns false for null or undefined.
 * @param answer A decoded answer, `null`, or a missing Map result (`undefined`).
 * @returns `true` when `answer` is a `ScoreAnswer`.
 */
export function isScoreAnswer(answer: Answer | null | undefined): answer is ScoreAnswer {
    return answer !== null && answer !== undefined && answer.type === "score";
}

function evaluateSingle(state: unknown, question: Question, requestedModel: string | undefined): Answer {
    const questions = new Map<string, Question>();
    questions.set("answer", question);
    const model = requestedModel ?? "jev-latest";
    const result = evaluate({ state: state, questions: questions, model: model });
    const answer = result.answers.get("answer");
    if (answer === undefined) throw invalidResponse();
    return answer;
}

function evaluate(request: BatchRequest): BatchResponse {
    const body = buildRequestBody(request);
    const response = post("https://api.typesafe.ai/v1/systemone", body, authHeaders());
    if (!response.ok) throw typesafeHttpError(response.status, response.headers.get("retry-after") ?? null);
    return decodeResponse(response.body, request.questions);
}

/**
 * Build and validate the payload without credentials or HTTP.
 * @param request The batch request to serialize.
 * @returns The JSON request body.
 */
function buildRequestBody(request: BatchRequest): string {
    requireDescription(request.state, "state");
    const model = request.model ?? "jev-latest";
    if (model.trim().length === 0) throw invalidArgument("model cannot be blank");
    if (request.questions.size === 0) throw invalidArgument("questions cannot be empty");
    for (const [id, question] of request.questions) {
        if (id.trim().length === 0) throw invalidArgument("question IDs cannot be blank");
        validateQuestion(question);
    }
    const fields: string[] = [];
    for (const [id, question] of request.questions) fields.push(field(id, questionJson(question)));
    return "{" + field("state", jsonValue(request.state)) + "," + field("model", JSON.stringify(model)) +
        ",\"questions\":{" + fields.join(",") + "}}";
}

/**
 * Parse typed answers and verify they match the requested IDs and criteria.
 * @param body The raw JSON response body.
 * @param questions The questions that were sent, used to verify the answers.
 * @returns The decoded batch response.
 */
function decodeResponse(body: string, questions: Map<string, Question>): BatchResponse {
    try {
        const wire = JSON.parse(body) as { model: string; answers: unknown; usage: Usage };
        const answers = new Map<string, Answer>();
        requireObject(wire.answers);
        for (const [id, value] of Object.entries(wire.answers)) answers.set(id, decodeAnswer(value));
        const response: BatchResponse = { model: wire.model, answers: answers, usage: wire.usage };
        if (response.model.trim().length === 0 || response.answers.size !== questions.size) throw invalidResponse();
        requireCount(response.usage.input_tokens);
        requireCount(response.usage.output_tokens);
        for (const [id, question] of questions) {
            const answer = response.answers.get(id);
            if (answer === undefined || answer.type !== question.type) throw invalidResponse();
            validateAnswer(answer, question);
        }
        return response;
    } catch (cause) {
        throw invalidResponse();
    }
}

/**
 * Map HTTP failures without exposing request state, provider bodies, or credentials.
 * @param status The HTTP status code of the failed response.
 * @param retryAfter The `retry-after` response header value, or `null` when absent.
 * @returns A `TypeSafeError` carrying a code derived from the status.
 */
function typesafeHttpError(status: number, retryAfter: string | null): TypeSafeError {
    let code = "http_error";
    if (status === 400 || status === 422) code = "invalid_request";
    else if (status === 401) code = "unauthorized";
    else if (status === 403) code = "forbidden";
    else if (status === 429) code = "rate_limited";
    else if (status === 529) code = "overloaded";
    return new TypeSafeError(code, "TypeSafe request failed with HTTP " + status.toString(), status, retryAfter);
}

function questionJson(question: Question): string {
    const fields = [field("type", JSON.stringify(question.type)), field("instructions", jsonValue(question.instructions))];
    if (question.type === "score") fields.push(field("criteria", JSON.stringify(question.criteria)));
    else if (question.criteria !== undefined) fields.push(field("criteria", criteriaJson(question.criteria)));
    return "{" + fields.join(",") + "}";
}

function criteriaJson(criteria: Map<string, unknown>): string {
    const fields: string[] = [];
    for (const [key, value] of criteria) fields.push(field(key, jsonValue(value)));
    return "{" + fields.join(",") + "}";
}

function jsonValue(value: unknown): string {
    const encoded = JSON.stringify(value);
    if (encoded === undefined) throw invalidArgument("value must have a JSON representation");
    return encoded;
}

function field(name: string, json: string): string { return JSON.stringify(name) + ":" + json; }

function decodeAnswer(value: unknown): Answer {
    const tag = value as { type: string };
    if (tag.type === "noul") return value as NoulAnswer;
    if (tag.type === "choice") {
        const wire = value as { type: string; choice: string; probabilities: unknown; confidence: number };
        return { type: "choice", choice: wire.choice, probabilities: decodeProbabilities(wire.probabilities), confidence: wire.confidence };
    }
    if (tag.type !== "score") throw invalidResponse();
    const wire = value as { type: string; score: number; probabilities: unknown; legend: unknown; confidence: number };
    requireObject(wire.legend);
    const legend = new Map<string, unknown>();
    for (const [key, description] of Object.entries(wire.legend)) legend.set(key, description);
    return { type: "score", score: wire.score, probabilities: decodeProbabilities(wire.probabilities), legend: legend, confidence: wire.confidence };
}

function decodeProbabilities(value: unknown): Map<string, number> {
    requireObject(value);
    const probabilities = new Map<string, number>();
    for (const [key, probability] of Object.entries(value)) probabilities.set(key, probability as number);
    return probabilities;
}

function requireObject(value: unknown): void {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw invalidResponse();
}

function authHeaders(): Map<string, string> {
    const key = secrets.get("TYPESAFE_AI_KEY");
    if (key === undefined || key.trim().length === 0) {
        throw new TypeSafeError("missing_credentials", "Bind TYPESAFE_AI_KEY in the blueprint before using TypeSafe");
    }
    const headers = new Map<string, string>();
    headers.set("Authorization", "Bearer " + key);
    headers.set("Content-Type", "application/json");
    headers.set("Accept", "application/json");
    return headers;
}

function validateQuestion(question: Question): void {
    requireDescription(question.instructions, "instructions");
    if (question.type === "score") {
        const levels = question.criteria;
        if (levels.length < 2 || levels.length > 10) throw invalidArgument("score requires 2–10 levels");
        for (const level of levels) requireDescription(level, "score level");
        return;
    }
    const criteria = question.criteria;
    if (criteria === undefined) return;
    if (question.type === "choice") {
        if (criteria.size < 1 || criteria.size > 255) throw invalidArgument("choice requires 1–255 options");
    }
    for (const [name, description] of criteria) {
        if (name.trim().length === 0) throw invalidArgument("criteria keys cannot be blank");
        if (question.type === "noul" && name !== "true" && name !== "false") {
            throw invalidArgument("noul criteria keys must be true or false");
        }
        if (description !== null || question.type === "noul") requireDescription(description, "criterion");
    }
}

function requireDescription(value: unknown, name: string): void {
    if (value === null || (typeof value !== "string" && typeof value !== "object")) {
        throw invalidArgument(name + " must be a string, JSON object, or array");
    }
    if (typeof value === "string" && value.trim().length === 0) throw invalidArgument(name + " cannot be blank");
}

function validateAnswer(answer: Answer, question: Question): void {
    if (answer.type === "noul") { requireProbability(answer.noul); return; }
    requireProbability(answer.confidence);
    let expected = new Map<string, unknown>();
    if (question.type === "choice") expected = question.criteria;
    else if (question.type === "score") {
        for (let i = 0; i < question.criteria.length; i += 1) expected.set(i.toString(), question.criteria[i]);
    }
    if (answer.probabilities.size !== expected.size) throw invalidResponse();
    let total = 0;
    for (const [key, probability] of answer.probabilities) {
        if (!expected.has(key)) throw invalidResponse();
        requireProbability(probability);
        total += probability;
    }
    if (Math.abs(total - 1) > 0.01) throw invalidResponse();
    if (answer.type === "choice") {
        if (!expected.has(answer.choice)) throw invalidResponse();
        return;
    }
    if (!Number.isFinite(answer.score) || answer.score < 0 || answer.score > expected.size - 1) throw invalidResponse();
    if (answer.legend.size !== expected.size) throw invalidResponse();
    for (const [key, description] of answer.legend) {
        if (!expected.has(key)) throw invalidResponse();
        requireDescription(description, "legend");
    }
}

function requireProbability(value: number): void {
    if (!Number.isFinite(value) || value < 0 || value > 1) throw invalidResponse();
}
function requireCount(value: number): void {
    if (!Number.isFinite(value) || value < 0 || value !== Math.floor(value)) throw invalidResponse();
}
function invalidArgument(message: string): TypeSafeError { return new TypeSafeError("invalid_argument", message); }
function invalidResponse(): TypeSafeError { return new TypeSafeError("invalid_response", "TypeSafe returned an invalid or mismatched response"); }
