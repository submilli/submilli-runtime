import { search, scrape, SearchOptions } from "@submilli/firecrawl";

class MutatingSchema {
    calls: number = 0;

    constructor(private readonly tags: string[], private readonly options: SearchOptions) {}

    toJson(): string {
        this.calls++;
        this.tags.push("");
        this.options.limit = 100;
        this.options.scrapeOptions!.maxAge = -1;
        return '{"type":"object"}';
    }
}

function main(): string {
    let reached = false;
    try { search("q", { limit: 2, scrapeOptions: {} }); } catch (error) {
        reached = error.name === "PermissionDeniedError" && error.message.includes("secrets.get") && error.message.includes("@submilli/firecrawl");
    }
    assert(reached, "explicit broad search scraping grants reach the credential boundary");
    const tags = ["article"];
    const options: SearchOptions = { limit: 2, scrapeOptions: { includeTags: tags, maxAge: 0 } };
    const schema = new MutatingSchema(tags, options);
    options.scrapeOptions!.json = { schema: schema };
    let snapshotted = false;
    try { search("q", options); } catch (error) {
        snapshotted = error.name === "PermissionDeniedError" && error.message.includes("secrets.get");
    }
    assert(schema.calls === 1 && tags.length === 2 && options.limit === 100, "schema serialization ran caller code once");
    assert(snapshotted, "search uses option and tag snapshots despite schema serialization mutating caller inputs");
    let denied = false;
    try { scrape("https://example.com"); } catch (error) {
        denied = error.name === "PermissionDeniedError" && error.message.includes("firecrawl.dev/scrape") && error.message.includes("main");
    }
    assert(denied, "search scraping does not grant ordinary scrape access");
    return "search content capability checks passed";
}
