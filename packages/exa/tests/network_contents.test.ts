import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { getContents } from "@submilli/exa";

function main(): void {
    const key = secrets.get("EXA_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: EXA_API_KEY is not bound");
        return;
    }
    label("live known-URL extraction returns content and statuses");
    const response = getContents(["https://exa.ai/docs"]);
    assert(response.results.length > 0 && response.statuses.length > 0, "extracted source and outcome");
    let success = false;
    for (const status of response.statuses) {
        if (status.status === "success") success = true;
    }
    assert(success, "successful extraction");
    let passages = 0;
    for (const result of response.results) {
        assert(result.url.length > 0, "source URL");
        for (const highlight of result.highlights) {
            if (highlight.length > 0) passages += 1;
        }
    }
    assert(passages > 0, "usable extracted highlights");
}
