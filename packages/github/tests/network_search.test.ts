// Anonymous public API probes: no token or private repository is needed.
import { label } from "submilli:test";
import { get } from "submilli:http";
import { encodeQuery } from "submilli:url";
import { GitHubError, searchIssues } from "@submilli/github";

interface SearchItem { repository_url: string; }
interface SearchResponse { items: SearchItem[]; }

function main(): void {
    // These forms were verified to add repositories despite the appended repo qualifier.
    for (const form of ['""repo:github/docs', "owner:github"]) {
        label("live GitHub scope parser: " + form);
        const params = new Map<string, string>();
        params.set("q", form + " repo:octocat/Hello-World is:issue");
        params.set("per_page", "10");
        const headers = new Map<string, string>();
        headers.set("Accept", "application/vnd.github+json");
        headers.set("User-Agent", "submilli-search-scope-test");
        const response = get("https://api.github.com/search/issues?" + encodeQuery(params), headers);
        assert(response.status === 200, "public GitHub search must succeed; got " + response.status.toString());
        const data = response.json() as SearchResponse;
        let outside = false;
        for (const item of data.items) {
            if (item.repository_url !== "https://api.github.com/repos/octocat/Hello-World") outside = true;
        }
        assert(outside, "GitHub recognizes the extra scope in " + form);
        let refused = false;
        try {
            searchIssues({ owner: "octocat", name: "Hello-World" }, form);
        } catch (error: GitHubError) {
            refused = error.code === "unsafe_search_query";
        }
        assert(refused, "package refuses the live scope-changing form");
    }
}
