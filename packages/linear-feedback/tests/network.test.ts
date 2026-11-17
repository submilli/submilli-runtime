// Live: files one [TEST] issue. Runs only with FEEDBACK_LINEAR_LIVE=true and both secrets bound.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { submitFeedback } from "@submilli/linear-feedback";

function main(): void {
    if (secrets.get("FEEDBACK_LINEAR_LIVE") !== "true") {
        label("skip: live feedback filing is disabled");
        return;
    }
    label("files one [TEST] issue in the configured team");
    const filed = submitFeedback({
        original: "live package test, please archive",
        summary: "Live package test of @submilli/linear-feedback",
        confirmed: true,
        kind: "other",
        test: true,
        context: "submilli build test -p @submilli/linear-feedback",
    });
    assert(filed.title.startsWith("[TEST] ") && filed.identifier.length > 0 && filed.url.length > 0, "filed");
    label("filed " + filed.identifier + " in " + filed.state + "; archive it");
}
