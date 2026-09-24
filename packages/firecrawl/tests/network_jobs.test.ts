import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { readText } from "submilli:fs";
import { startBatch, startCrawl, getJob, getJobErrors, cancelJob, downloadJobPage, normalizeJobPageJson, JobKind } from "@submilli/firecrawl";

function inspectAndCancel(kind: JobKind, id: string): void {
    try {
        const first = getJob(kind, id);
        assert(first.status.length > 0 && first.total >= 0, "job status");
        if (first.next !== null) {
            const second = getJob(kind, id, first.next);
            assert(second.status.length > 0, "one explicit next page");
        }
        const saved = downloadJobPage(kind, id, "/" + kind + "-page.json");
        assert(saved.status === 200 && saved.bytesWritten > 0, "VFS result envelope");
        const body = readText(saved.path);
        assert(body !== null, "download exists");
        const downloaded = normalizeJobPageJson(body!, kind, id);
        assert(downloaded.status.length > 0, "downloaded status and pagination validate");
        const errors = getJobErrors(kind, id);
        assert(errors.errors.length >= 0 && errors.robotsBlocked.length >= 0, "failure lists");
    } finally {
        // Only cancel the disposable job created by this test, even if inspection fails.
        assert(cancelJob(kind, id).status === "cancelled", "cancel acknowledgement");
    }
}
function main(): void {
    const key = secrets.get("FIRECRAWL_API_KEY");
    const url = secrets.get("FIRECRAWL_TEST_URL");
    if (key === null || key.trim().length === 0 || url === null || url.trim().length === 0) {
        label("skip: bind FIRECRAWL_API_KEY and FIRECRAWL_TEST_URL to create disposable jobs");
        return;
    }
    label("live bounded jobs: submit, inspect one page, inspect errors, cancel");
    const batch = startBatch([url]);
    inspectAndCancel("batch", batch.id);
    const crawl = startCrawl(url, { limit: 1, maxDiscoveryDepth: 0, sitemap: "skip" });
    inspectAndCancel("crawl", crawl.id);
}
