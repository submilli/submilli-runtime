import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { batch, choiceQuestion, noulQuestion, scoreQuestion, Question, choice, noul, score,
    isNoulAnswer, isChoiceAnswer, isScoreAnswer } from "@submilli/typesafe";

function main(): void {
    const key = secrets.get("TYPESAFE_AI_KEY");
    if (key === null || key.trim().length === 0) {
        label("skip: TYPESAFE_AI_KEY is not bound");
        return;
    }
    label("one live request returns all three typed judgments");
    const categories = new Map<string, unknown>();
    categories.set("billing", "Charges, invoices, or refunds");
    categories.set("technical", "Broken software");
    categories.set("other", "Neither category");
    const questions = new Map<string, Question>();
    questions.set('category"id', choiceQuestion({ question: "What is the customer's request in ticket.text about?" }, categories));
    questions.set("refund", noulQuestion("Does the customer explicitly request a refund?"));
    questions.set("urgency", scoreQuestion("How time-sensitive is this request?", [
        "No deadline or time pressure mentioned", "Explicit deadline within the next week", "Immediate action requested today"
    ]));
    const response = batch({ state: { ticket: { text: "I was charged twice for one order. Please refund the duplicate payment today." } }, questions: questions });
    const category = response.answers.get('category"id');
    if (!isChoiceAnswer(category)) throw new Error("Unexpected answer type");
    const refund = response.answers.get("refund");
    if (!isNoulAnswer(refund)) throw new Error("Unexpected answer type");
    const urgency = response.answers.get("urgency");
    if (!isScoreAnswer(urgency)) throw new Error("Unexpected answer type");
    assert(category.probabilities.size === 3 && categories.has(category.choice), "all choice options and selected key returned");
    assert(category.confidence >= 0 && category.confidence <= 1, "choice confidence");
    assert(JSON.stringify(Array.from(category.probabilities)).includes(category.choice), "documented map serialization");
    assert(refund.noul >= 0 && refund.noul <= 1, "yes probability");
    assert(urgency.score >= 0 && urgency.score <= 2 && urgency.legend.size === 3, "score uses level indices");
    assert(urgency.probabilities.size === 3 && urgency.confidence >= 0 && urgency.confidence <= 1, "score uncertainty");
    assert(response.answers.size === questions.size, "question IDs preserved");
    assert(response.usage.input_tokens > 0 && response.usage.output_tokens >= 0 && response.model.length > 0, "usage and model");

    label("single-question functions return concrete answers without a question map");
    const text = "Please refund the duplicate payment today.";
    const singleRefund = noul(text, "Does the customer request a refund?");
    assert(singleRefund.type === "noul" && singleRefund.noul >= 0 && singleRefund.noul <= 1, "direct noul");
    const singleCategory = choice(text, "What is the request about?", categories, { model: response.model });
    assert(singleCategory.type === "choice" && categories.has(singleCategory.choice), "direct choice with pinned model");
    assert(singleCategory.probabilities.size === 3 && singleCategory.confidence >= 0, "choice uncertainty");
    const singleUrgency = score(text, "How time-sensitive is this request?", [
        "No time pressure mentioned", "Immediate action requested today"
    ]);
    assert(singleUrgency.type === "score" && singleUrgency.score >= 0 && singleUrgency.score <= 1, "direct score");
    assert(singleUrgency.legend.size === 2 && singleUrgency.probabilities.size === 2, "score distributions");

}
