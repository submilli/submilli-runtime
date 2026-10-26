// Run through `submilli run --blueprint read-once.yaml`, not the allow-all build test runner.
import { AttributeValue, AttributeValueType, CompanyInput, PersonInput, createRecord, updateRecord, upsertCompany, upsertPerson } from "@submilli/attio";

const DEAL = "8a1b2c3d-1111-4222-8333-944455556666";

// `main` controls these objects while the package reads them. Each checked field answers
// an allowed value to its first read and a forbidden one to every later read, so a package
// that reads it for `check` and again for the request approves one value and sends another.
class FlippingWrite implements AttributeValue {
    private reads: number = 0;
    type: AttributeValueType = "member";
    email: string = "someone@example.com";

    get attribute(): string {
        this.reads += 1;
        return this.reads === 1 ? "name" : "owner";
    }

    // `AttributeValue` fields are writable, which a getter alone does not satisfy.
    set attribute(value: string) {}

    attributeReads(): number {
        return this.reads;
    }
}

class FlippingCompany implements CompanyInput {
    private reads: number = 0;

    get domain(): string {
        this.reads += 1;
        return this.reads === 1 ? "acme.com" : "victim.com";
    }

    set domain(value: string) {}

    domainReads(): number {
        return this.reads;
    }
}

class FlippingPerson implements PersonInput {
    private reads: number = 0;

    get email(): string {
        this.reads += 1;
        return this.reads === 1 ? "dana@acme.com" : "ceo@victim.com";
    }

    set email(value: string) {}

    emailReads(): number {
        return this.reads;
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
    // This denial proves the business check passed, without reading a key or calling Attio.
    deniedAt("secrets.get", "@submilli/attio", action);
}

function main(): string {
    // The request is built before the credential boundary and never sent, so it cannot be
    // inspected here. One read is the proof instead: the package holds a single value,
    // `check` approved it, and any further read answers the forbidden one.
    const update = new FlippingWrite();
    reachesCredentialBoundary(() => { updateRecord("deals", DEAL, [update]); });
    assert(update.attributeReads() === 1, "updateRecord read the attribute " + update.attributeReads().toString() + " times, expected 1");
    deniedAt("attio.com/records.update", "main", () => { updateRecord("deals", DEAL, [update]); });

    const create = new FlippingWrite();
    reachesCredentialBoundary(() => { createRecord("deals", { values: [create] }); });
    assert(create.attributeReads() === 1, "createRecord read the attribute " + create.attributeReads().toString() + " times, expected 1");

    const company = new FlippingCompany();
    reachesCredentialBoundary(() => { upsertCompany(company); });
    assert(company.domainReads() === 1, "upsertCompany read the domain " + company.domainReads().toString() + " times, expected 1");
    deniedAt("attio.com/companies.upsert", "main", () => { upsertCompany(company); });

    const person = new FlippingPerson();
    reachesCredentialBoundary(() => { upsertPerson(person); });
    assert(person.emailReads() === 1, "upsertPerson read the email " + person.emailReads().toString() + " times, expected 1");
    deniedAt("attio.com/people.upsert", "main", () => { upsertPerson(person); });
    return "read-once checks passed";
}
