import { label } from "submilli:test";
import { ClefError, Question, batch, choice, choiceQuestion, noul, noulQuestion, score,
    scoreQuestion } from "@submilli/cloudflare-clef";

function expectCode(code: string, action: () => void): void {
    let matched = false;
    try { action(); } catch (cause) {
        if (cause instanceof ClefError) matched = cause.code === code && cause.status === 0;
    }
    assert(matched, "expected " + code);
}

function questions(): Map<string, Question> {
    const options = new Map<string, unknown>();
    options.set("match", "Relevant to the query");
    options.set("none", null);
    const result = new Map<string, Question>();
    result.set("pick", choiceQuestion("Which candidate fits?", options));
    result.set("present", noulQuestion("Is an answer present?"));
    result.set("quality", scoreQuestion("How useful is the evidence?", ["Unrelated", "Direct answer"]));
    return result;
}

function main(): void {
    label("builders produce the shared question types");
    const built = questions();
    assert(built.get("pick")?.type === "choice" && built.get("present")?.type === "noul", "builders");

    label("invalid requests fail before credentials or HTTP");
    expectCode("invalid_argument", () => { batch({ state: null, questions: questions() }); });
    expectCode("invalid_argument", () => { batch({ state: "text", questions: new Map<string, Question>() }); });
    expectCode("invalid_argument", () => { noul("text", " "); });
    expectCode("invalid_argument", () => { score("text", "q", ["only"]); });
    const options = new Map<string, unknown>();
    options.set("match", null);
    expectCode("invalid_argument", () => { choice("text", "q", options, { model: " " }); });

    label("only Clef models hosted on Workers AI are accepted");
    expectCode("invalid_argument", () => { batch({ state: "text", questions: questions(), model: "jev-latest" }); });
    expectCode("invalid_argument", () => { noul("text", "q", undefined, { model: "@cf/cloudflare/clef" }); });
    expectCode("invalid_argument", () => { noul("text", "q", undefined, { model: "CLEF" }); });
    expectCode("invalid_argument", () => { noul("text", "q", undefined, { model: "" }); });

    label("missing credentials fail before HTTP");
    expectCode("missing_credentials", () => { noul("text", "Is a refund requested?"); });
    expectCode("missing_credentials", () => { batch({ state: "text", questions: questions(), model: "clef-flash" }); });
}
