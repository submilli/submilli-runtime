import { label } from "submilli:test";
import { Question, TypeSafeError, choiceQuestion, noulQuestion, scoreQuestion, batch, choice, noul, score, Answer,
    isNoulAnswer, isChoiceAnswer, isScoreAnswer } from "@submilli/typesafe";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        if (cause instanceof TypeSafeError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function questions(): Map<string, Question> {
    const options = new Map<string, unknown>();
    options.set("yes", "Relevant to the query");
    options.set("none", null);
    const result = new Map<string, Question>();
    result.set("pick", choiceQuestion("Which candidate fits?", options));
    result.set("present", noulQuestion("Is an answer present?"));
    result.set("quality", scoreQuestion("How useful is the evidence?", ["Unrelated", "Direct answer"]));
    return result;
}

function main(): void {
    testAnswerGuards();

    label("question builders preserve structured instructions and criteria");
    const instructions = { question: 'Does the text request a "refund"?', source: "ticket.text" };
    const rubric = new Map<string, unknown>();
    rubric.set("true", { definition: "Explicit request", examples: ["Please refund me"] });
    rubric.set("false", "No request");
    const refund = noulQuestion(instructions, rubric);
    assert(refund.type === "noul", "noul discriminator");
    assert(JSON.stringify(refund.instructions) === JSON.stringify(instructions), "structured instructions");
    assert(refund.criteria !== undefined && refund.criteria.get("false") === "No request", "optional rubric");
    assert(noulQuestion("Is a refund requested?").criteria === undefined, "rubric is optional");
    assert(noulQuestion("Is a refund requested?", undefined).criteria === undefined, "undefined rubric is omitted");
    const options = new Map<string, unknown>();
    options.set("match", "Relevant to the query");
    options.set("none", null);
    const selected = choiceQuestion("Which candidate fits?", options);
    assert(selected.type === "choice" && selected.criteria.has("none"), "no-match option preserved");
    assert(selected.criteria.get("none") === null, "option may have no description");
    const rated = scoreQuestion("How useful is the evidence?", ["Unrelated", { description: "Direct answer" }]);
    assert(rated.type === "score" && rated.criteria.length === 2, "ordered structured score levels");

    label("invalid questions and states fail before credentials or HTTP");
    expectCode("invalid_argument", () => { batch({ state: null, questions: questions() }); });
    expectCode("invalid_argument", () => { batch({ state: 5, questions: questions() }); });
    expectCode("invalid_argument", () => { batch({ state: "text", questions: new Map<string, Question>() }); });
    expectCode("invalid_argument", () => { batch({ state: "text", questions: questions(), model: " " }); });
    expectCode("invalid_argument", () => { noulQuestion(" "); });
    expectCode("invalid_argument", () => { noulQuestion(true); });
    expectCode("invalid_argument", () => { scoreQuestion("q", ["only"]); });
    expectCode("invalid_argument", () => { scoreQuestion("q", ["valid", null]); });
    expectCode("invalid_argument", () => { choiceQuestion("q", new Map<string, unknown>()); });
    const bad = new Map<string, unknown>();
    bad.set("maybe", "yes");
    expectCode("invalid_argument", () => { noulQuestion("q", bad); });
    bad.clear();
    for (let i = 0; i < 255; i += 1) bad.set(i.toString(), null);
    choiceQuestion("q", bad);
    bad.set("overflow", null);
    expectCode("invalid_argument", () => { choiceQuestion("q", bad); });
    const levels: unknown[] = [];
    for (let i = 0; i < 10; i += 1) levels.push("level " + i.toString());
    scoreQuestion("q", levels);
    levels.push("overflow");
    expectCode("invalid_argument", () => { scoreQuestion("q", levels); });
    const blankId = questions();
    blankId.set(" ", noulQuestion("q"));
    expectCode("invalid_argument", () => { batch({ state: "text", questions: blankId }); });

    label("undefined descriptions fail before credentials or HTTP");
    expectCode("invalid_argument", () => { noul(undefined, "Question"); });
    expectCode("invalid_argument", () => { noul("state", undefined); });

    label("single-question calls validate before credentials or HTTP");
    expectCode("invalid_argument", () => { noul(null, "Is a refund requested?"); });
    expectCode("invalid_argument", () => { noul("text", " "); });
    expectCode("invalid_argument", () => { noul("text", "q", undefined, { model: " " }); });
    const singleOptions = new Map<string, unknown>();
    singleOptions.set("match", null);
    expectCode("invalid_argument", () => { choice(null, "q", singleOptions); });
    expectCode("invalid_argument", () => { choice("text", "q", new Map<string, unknown>()); });
    expectCode("invalid_argument", () => { choice("text", "q", singleOptions, { model: " " }); });
    expectCode("invalid_argument", () => { score(null, "q", ["low", "high"]); });
    expectCode("invalid_argument", () => { score("text", "q", ["only"]); });
    expectCode("invalid_argument", () => { score("text", "q", ["low", "high"], { model: " " }); });

    label("batch revalidates questions after caller mutations");
    const mutated = questions();
    const invalid: Question = { type: "score", instructions: "How useful?", criteria: ["Only one level"] };
    mutated.set("invalid", invalid);
    expectCode("invalid_argument", () => { batch({ state: "text", questions: mutated }); });
    const criteria = new Map<string, unknown>();
    criteria.set("match", null);
    const mutable = choiceQuestion("Which candidate fits?", criteria);
    criteria.clear();
    const mutableQuestions = new Map<string, Question>();
    mutableQuestions.set("pick", mutable);
    expectCode("invalid_argument", () => { batch({ state: "text", questions: mutableQuestions }); });
}

function testAnswerGuards(): void {
    label("exported type guards narrow nullable answers across package imports");
    const probabilities = new Map<string, number>();
    probabilities.set("match", 1);
    const levels = new Map<string, number>();
    levels.set("0", 0.25);
    levels.set("1", 0.75);
    const legend = new Map<string, unknown>();
    legend.set("0", "Unrelated");
    legend.set("1", "Relevant");
    const answers = new Map<string, Answer>();
    answers.set("noul", { type: "noul", noul: 0.9 });
    answers.set("choice", { type: "choice", choice: "match", probabilities: probabilities, confidence: 1 });
    answers.set("score", { type: "score", score: 0.75, probabilities: levels, legend: legend, confidence: 0.4 });
    const yes = answers.get("noul");
    const picked = answers.get("choice");
    const graded = answers.get("score");
    const missing = answers.get("missing");
    assert(missing === undefined, "Map misses preserve undefined");
    assert(!isNoulAnswer(null) && !isChoiceAnswer(null) && !isScoreAnswer(null), "null is not an answer");
    assert(!isNoulAnswer(missing) && !isChoiceAnswer(missing) && !isScoreAnswer(missing), "a missing answer is undefined");
    assert(!isChoiceAnswer(yes) && !isScoreAnswer(yes), "noul rejects other guards");
    assert(!isNoulAnswer(picked) && !isScoreAnswer(picked), "choice rejects other guards");
    assert(!isNoulAnswer(graded) && !isChoiceAnswer(graded), "score rejects other guards");
    if (!isNoulAnswer(yes)) throw new Error("Expected NoulAnswer");
    assert(yes.noul === 0.9, "negative guard narrows noul");
    if (!isChoiceAnswer(picked)) throw new Error("Expected ChoiceAnswer");
    assert(picked.choice === "match" && picked.probabilities.get("match") === 1, "negative guard narrows choice");
    if (!isScoreAnswer(graded)) throw new Error("Expected ScoreAnswer");
    assert(graded.score === 0.75 && graded.legend.get("1") === "Relevant", "negative guard narrows score");
    let total = 0;
    for (const answer of answers.values()) {
        if (isNoulAnswer(answer)) total += answer.noul;
        else if (isChoiceAnswer(answer)) total += answer.confidence;
        else if (isScoreAnswer(answer)) total += answer.score;
    }
    assert(Math.abs(total - 2.65) < 0.000001, "positive branches narrow each answer variant");
}
