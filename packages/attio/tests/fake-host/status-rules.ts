// Run by `attio_status_rules_hold_against_a_fake_attio` in crates/submilli/tests/package_policy.rs.
// The package copy talks to a fake Attio, so every check runs with the values Attio's answers
// give it, under the real policy engine. The test also asserts which writes reached the fake.
import { AttioError, createRecord, setStatus, updateRecord } from "@submilli/attio";

const DEAL = "8a1b2c3d-1111-4222-8333-944455556666";

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

function rejectedAs(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (error: AttioError) {
        outcome = error.code;
    } catch (error) {
        outcome = error.name + ": " + error.message;
    }
    assert(outcome === code, "expected " + code + ", got " + outcome);
}

function main(): string {
    // Allowed: the stage of a deal, by a title in another case. The fake answers with the
    // record in Lead, so the check sees the canonical title and the previous status.
    const won = setStatus("deals", DEAL, "stage", "won 🎉");
    assert(won.changed, "the stage changed");
    assert(won.status === "Won 🎉", "canonical title, got " + won.status);
    assert(won.previousStatus === "Lead", "previous status, got " + won.previousStatus);

    // Denied: another status attribute of the same deal. The rule allows only `stage`.
    deniedAt("attio.com/records.setStatus", "main", () => { setStatus("deals", DEAL, "priority", "High"); });

    // A status attribute cannot be written as a plain value, in any position.
    rejectedAs("status_attribute", () => {
        updateRecord("deals", DEAL, [
            { attribute: "name", type: "text", text: "Acme pilot" },
            { attribute: "stage", type: "select", option: "Won 🎉" },
        ]);
    });
    rejectedAs("status_attribute", () => {
        createRecord("deals", {
            values: [
                { attribute: "name", type: "text", text: "Acme pilot" },
                { attribute: "priority", type: "select", option: "High" },
            ],
            status: { attribute: "stage", title: "Lead" },
        });
    });
    // A company created with a status is refused by the create-only rule, once the
    // statuses are read and records.create sees the status it would set.
    deniedAt("attio.com/records.create", "main", () => {
        createRecord("companies", {
            values: [{ attribute: "domains", type: "domain", domain: "acme.com" }],
            status: { attribute: "stage", title: "Lead" },
        });
    });
    return "status rules hold against the fake Attio";
}
