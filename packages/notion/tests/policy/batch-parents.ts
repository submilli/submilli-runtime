// Run through `submilli run --blueprint batch-parents.yaml`, not the allow-all build test runner.
import { MovePageInput, NotionError, PageParent, createPage, createPages, movePage, movePages } from "@submilli/notion";

const ALLOWED_PARENT = "11111111-1111-4111-8111-111111111111";
const BLOCKED_PARENT = "22222222-2222-4222-8222-222222222222";
const PAGE = "33333333-3333-4333-8333-333333333333";
const OTHER_PAGE = "44444444-4444-4444-8444-444444444444";

// `main` controls this object while the package reads it. `type` answers "page_id" to its first
// read and "workspace" to every later one, so a package that reads it to resolve the parent ID
// and again for the request approves a move under one page and moves to the workspace.
class FlippingParent implements PageParent {
    id: string = ALLOWED_PARENT;
    private typeReadCount: number = 0;

    get type(): "page_id" | "data_source_id" | "workspace" {
        this.typeReadCount += 1;
        return this.typeReadCount === 1 ? "page_id" : "workspace";
    }

    // `PageParent` fields are writable, which a getter alone does not satisfy.
    set type(value: "page_id" | "data_source_id" | "workspace") {}

    typeReads(): number {
        return this.typeReadCount;
    }
}

function deniedAt(capability: string, caller: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: PermissionDeniedError) {
        outcome = error.caller + " denied at " + error.capability;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    const expected = caller + " denied at " + capability;
    assert(outcome === expected, "expected " + expected + ", got " + outcome);
}

function reachesCredentialBoundary(action: () => void): void {
    // This denial proves every element passed its check, without reading a token or calling Notion.
    deniedAt("secrets.get", "@submilli/notion", action);
}

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: NotionError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function under(parentId: string): PageParent {
    return { type: "page_id", id: parentId };
}

function readsParentTypeOnce(operation: string, call: (parent: FlippingParent) => void): void {
    const flipping = new FlippingParent();
    reachesCredentialBoundary(() => { call(flipping); });
    assert(flipping.typeReads() === 1, operation + " read the parent type " + flipping.typeReads().toString() + " times, expected 1");
}

function main(): string {
    deniedAt("submilli/notion.createPage", "main", () => { createPage({ parent: under(BLOCKED_PARENT) }); });
    reachesCredentialBoundary(() => { createPage({ parent: under(ALLOWED_PARENT) }); });
    // A batch creates each page as `createPage` does, under the same capability and rule.
    deniedAt("submilli/notion.createPage", "main", () => { createPages([{ parent: under(BLOCKED_PARENT) }, { parent: under(ALLOWED_PARENT) }]); });
    deniedAt("submilli/notion.createPage", "main", () => { createPages([{ parent: { type: "workspace" } }]); });
    reachesCredentialBoundary(() => { createPages([{ parent: under(ALLOWED_PARENT) }, { parent: under(ALLOWED_PARENT) }]); });
    // An invalid input anywhere in the batch is refused before the first page is checked or created.
    rejectedAs("invalid_parent", () => { createPages([{ parent: under(ALLOWED_PARENT) }, { parent: { type: "page_id" } }]); });

    deniedAt("submilli/notion.movePage", "main", () => { movePage(PAGE, under(BLOCKED_PARENT)); });
    reachesCredentialBoundary(() => { movePage(PAGE, under(ALLOWED_PARENT)); });
    // Every move of a batch is checked before the first page moves. With the blocked parent last,
    // checking as each page is moved would move the first page before the denial.
    const allowedMove: MovePageInput = { page: PAGE, parent: under(ALLOWED_PARENT) };
    const blockedMove: MovePageInput = { page: OTHER_PAGE, parent: under(BLOCKED_PARENT) };
    deniedAt("submilli/notion.movePages", "main", () => { movePages([allowedMove, blockedMove]); });
    deniedAt("submilli/notion.movePages", "main", () => { movePages([blockedMove, allowedMove]); });
    deniedAt("submilli/notion.movePages", "main", () => { movePages([{ page: PAGE, parent: { type: "workspace" } }]); });
    reachesCredentialBoundary(() => { movePages([allowedMove, { page: OTHER_PAGE, parent: under(ALLOWED_PARENT) }]); });

    readsParentTypeOnce("createPage", (parent: FlippingParent): void => { createPage({ parent: parent }); });
    readsParentTypeOnce("movePage", (parent: FlippingParent): void => { movePage(PAGE, parent); });
    readsParentTypeOnce("movePages", (parent: FlippingParent): void => { movePages([{ page: PAGE, parent: parent }]); });
    return "batch parent checks passed";
}
