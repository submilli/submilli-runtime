// The blueprint denies secrets.get, so accepted queries stop before credentials or HTTP.
import { GitHubError, RepositoryRef, searchCode, searchIssues, searchPullRequests } from "@submilli/github";

const REPOSITORY: RepositoryRef = { owner: "allowed-org", name: "repo.js" };

function main(): string {
    const searches: ((query: string) => void)[] = [
        (query: string): void => { searchCode(REPOSITORY, query); },
        (query: string): void => { searchIssues(REPOSITORY, query); },
        (query: string): void => { searchPullRequests(REPOSITORY, query); },
    ];
    const allowed = [
        "is:private bug", "is:public", "is:open", 'is:"private" bug', 'is:"public"', 'is:"open"', "is:closed", "is:prerelease",
        "is:issues", "priority", "word", '"repo:other/repo OR is:pr"',
        '"org:other user:other owner:other"', '"ｒｅｐｏ:other/repo"',
        '"bug report" is:private', "author:octocat sort:updated-desc",
    ];
    const refused = [
        "repo:other/repo", "org:other", "user:other", "owner:other", "OWNER:other",
        'na"me"repo:other/repo', '"phrase"repo:other/repo', "x,repo:other/repo",
        "x +repo:other/repo", "x-repo:other/repo", "ｒｅｐｏ:other/repo", "repo：other/repo",
        "ＯＷＮＥＲ：other", "is:pr", "is:issue", "is:pull-request", "-is:pr",
         'is:"pr"', 'is:"issue"', 'is:"pull-request"', 'type:"pr"', 'type:"issue"', 'type:"pull-request"',
        'ｉｓ："ｐｒ"', "(is:issue)", "x,is:pr", "ｉｓ：ｐｒ", "type:issue", "type:pr", "type:pull-request",
        "needle OR other", "needle,OR,other", 'needle"phrase"OR other', "needle ＯＲ other",
        '"unterminated repo:other/repo', '"phrase\\" repo:other/repo "tail"',
    ];
    const separators = [
        "\u00ad", "\u200e", "\u200f", "\u202a", "\u202b", "\u202c", "\u202d", "\u202e",
        "\u2061", "\u2062", "\u2063", "\u2064", "\u2066", "\u2067", "\u2068", "\u2069",
        "\u206a", "\u206b", "\u206c", "\u206d", "\u206e", "\u206f", "\u3164",
    ];
    for (const separator of separators) {
        refused.push("needle" + separator + "repo:other/repo");
        refused.push("needle" + separator + "OR" + separator + "other");
    }
    for (const search of searches) {
        for (const query of allowed) assertOutcome("@submilli/github denied at secrets.get", query, () => { search(query); });
        for (const query of refused) assertOutcome("unsafe_search_query", query, () => { search(query); });
    }
    return "search scope checks passed";
}

function assertOutcome(expected: string, query: string, action: () => void): void {
    const actual = outcomeOf(action);
    assert(actual === expected, JSON.stringify(query) + ": expected " + expected + ", got " + actual);
}

function outcomeOf(action: () => void): string {
    try {
        action();
    } catch (error: GitHubError) {
        return error.code;
    } catch (error: PermissionDeniedError) {
        return error.caller + " denied at " + error.capability;
    }
    return "no error";
}
