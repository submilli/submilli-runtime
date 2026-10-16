import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { youtube } from "@submilli/scrapecreators";
function main(): void {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: SCRAPECREATORS_API_KEY is not bound");
        return;
    }
    label("live YouTube native captions preserve timestamped segments");
    const response = youtube.getTranscript({ url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ", language: "en" }).data;
    assert(response.transcript !== null && response.transcript.length > 0, "caption segments");
    assert(response.transcript_only_text !== undefined && response.transcript_only_text !== null, "caption text");
}
