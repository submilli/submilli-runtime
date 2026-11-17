import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { Question, batch, choice, choiceQuestion, isChoiceAnswer, isNoulAnswer, isScoreAnswer, noul, noulQuestion,
    score, scoreQuestion } from "@submilli/cloudflare-clef";

function hasSecret(name: string): boolean {
    const value = secrets.get(name);
    return value !== undefined && value.trim().length > 0;
}

function main(): void {
    if (!hasSecret("CLOUDFLARE_ACCOUNT_ID") || !hasSecret("CLOUDFLARE_API_TOKEN")) {
        label("skip: CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_API_TOKEN are not bound");
        return;
    }
    const categories = new Map<string, unknown>();
    categories.set("billing", "Charges, invoices, or refunds");
    categories.set("technical", "Broken software");
    categories.set("other", "Neither category");
    for (const model of ["clef", "clef-flash"]) {
        label(model + ": one live request returns all three typed judgments");
        const questions = new Map<string, Question>();
        questions.set("category", choiceQuestion({ question: "What is the customer's request in ticket.text about?" }, categories));
        questions.set("refund", noulQuestion("Does the customer explicitly request a refund?"));
        questions.set("urgency", scoreQuestion("How time-sensitive is this request?", [
            "No deadline or time pressure mentioned", "Explicit deadline within the next week", "Immediate action requested today"
        ]));
        const response = batch({
            state: { ticket: { text: "I was charged twice for one order. Please refund the duplicate payment today." } },
            questions: questions,
            model: model
        });
        const category = response.answers.get("category");
        const refund = response.answers.get("refund");
        const urgency = response.answers.get("urgency");
        if (!isChoiceAnswer(category)) throw new Error("Unexpected category answer");
        if (!isNoulAnswer(refund)) throw new Error("Unexpected refund answer");
        if (!isScoreAnswer(urgency)) throw new Error("Unexpected urgency answer");
        assert(category.choice === "billing" && category.probabilities.size === 3, "choice over every option");
        assert(refund.noul > 0.5, "refund is requested");
        assert(urgency.score >= 0 && urgency.score <= 2 && urgency.legend.size === 3, "score uses level indices");
        assert(response.usage.input_tokens > 0 && response.model.length > 0, "usage and model");
    }

    label("single-question functions return concrete answers");
    const text = "Please refund the duplicate payment today.";
    assert(noul(text, "Does the customer request a refund?").noul > 0.5, "direct noul");
    assert(choice(text, "What is the request about?", categories, { model: "clef-flash" }).choice === "billing", "direct choice");
    const urgency = score(text, "How time-sensitive is this request?", ["No time pressure mentioned", "Immediate action requested today"]);
    assert(urgency.score >= 0 && urgency.score <= 1 && urgency.probabilities.size === 2, "direct score");
}
