// Offline tests. The normalizers, builders and response mappers live in the
// package's internal modules, so these tests import them from `../src/`. The
// operations section checks that each operation validates its input before any
// request: invalid input fails with a local AttioError (status 0), and valid
// input stops at the token read with `missing_token`. That last section is
// skipped when ATTIO_API_KEY is bound, so this file never sends a request.
import { label } from "submilli:test";
import secrets from "submilli:secrets";
import {
    AttioError,
    AttioRecord,
    RecordValue,
    createNote,
    createRecord,
    findPersonByEmail,
    getRecord,
    listAttributes,
    listMembers,
    listNotes,
    listObjects,
    listStatuses,
    normalizeDomain as exportedNormalizeDomain,
    normalizeEmail as exportedNormalizeEmail,
    queryRecords,
    setStatus,
    updateRecord,
    upsertCompany,
    upsertPerson,
} from "@submilli/attio";
import {
    buildCompanyValues,
    buildNotesQuery,
    buildPersonValues,
    buildQueryBody,
    buildValues,
    groupValues,
    normalizeDomain,
    normalizeEmail,
    normalizeRecordId,
    normalizeSlug,
    PendingValue,
    PreparedCondition,
    prepareCondition,
    prepareSort,
    prepareValue,
    queryWindow,
    noteWindow,
    requirePlainAttributes,
    uniqueSlugs,
    ValueFields,
} from "../src/request";
import {
    Attribute,
    attributeFrom,
    AttributeList,
    currentStatus,
    failureFrom,
    findStatus,
    noteFrom,
    NoteEnvelope,
    objectFrom,
    ObjectList,
    pageOf,
    RecordEnvelope,
    recordFrom,
    retryAfterSeconds,
    Status,
} from "../src/response";

const RECORD = "8a1b2c3d-1111-4222-8333-944455556666";
const COMPANY = "0c0ffee0-aaaa-4bbb-8ccc-ddddeeeeffff";

function expectCode(code: string, action: () => void): void {
    let outcome = "no error";
    try {
        action();
    } catch (cause) {
        if (cause instanceof AttioError) outcome = cause.code + "/" + cause.status.toString();
        else outcome = "other error";
    }
    assert(outcome === code + "/0", "expected local AttioError " + code + ", got " + outcome);
}

function first(record: AttioRecord, slug: string): RecordValue {
    const list = record.values[slug];
    const value = list === undefined ? undefined : list.at(0);
    return value ?? { type: "", value: "" };
}

function fields(type: string): ValueFields {
    return {
        type: type,
        text: undefined,
        number: undefined,
        amount: undefined,
        checked: undefined,
        date: undefined,
        option: undefined,
        email: undefined,
        domain: undefined,
        targetObject: undefined,
        targetRecordId: undefined,
        firstName: undefined,
        lastName: undefined,
        fullName: undefined,
    };
}

function definition(slug: string, type: string, multiselect: boolean, writable: boolean): Attribute {
    return {
        attributeId: "",
        slug: slug,
        title: slug,
        description: "",
        type: type,
        isMultiselect: multiselect,
        isWritable: writable,
        isRequired: false,
        isUnique: false,
        currencyCode: "",
    };
}

function main(): void {
    label("emails are trimmed and lowercased, and malformed addresses refused");
    assert(normalizeEmail("  Dana@Acme.COM ") === "dana@acme.com", "email normalized");
    assert(normalizeEmail("first.last+tag@sub.acme.co.uk") === "first.last+tag@sub.acme.co.uk", "plus and subdomain kept");
    for (const bad of ["", "dana", "dana@", "@acme.com", "dana@acme", "dana@@acme.com", "da na@acme.com", "dana@acme..com", "dana@acme.com."]) {
        expectCode("invalid_email", () => { normalizeEmail(bad); });
    }
    // Invisible, look-alike and non-ASCII characters are refused, so an email rule
    // compares the spelling Attio stores; an internationalized domain uses punycode.
    for (const unicode of ["dana@competitor.\uff43om", "dana@competitor.com\u200b", "dana\u00a0x@acme.com", "dana@b\u00fccher.de"]) {
        expectCode("invalid_email", () => { normalizeEmail(unicode); });
    }
    assert(normalizeEmail("dana@xn--bcher-kva.de") === "dana@xn--bcher-kva.de", "punycode accepted");

    label("the package exports the normalizers, so a program can normalize before a query");
    assert(exportedNormalizeDomain("https://www.Acme.com/pricing") === "acme.com", "exported domain normalizer");
    assert(exportedNormalizeEmail(" Dana@Acme.COM ") === "dana@acme.com", "exported email normalizer");

    label("domains lose scheme, www, port, path, query and trailing dot");
    assert(normalizeDomain("https://www.Acme.com/pricing?x=1#top") === "acme.com", "URL reduced to its domain");
    assert(normalizeDomain(" ACME.com. ") === "acme.com", "case and trailing dot");
    assert(normalizeDomain("http://app.acme.co.uk:8443/login") === "app.acme.co.uk", "subdomain kept, port dropped");
    assert(normalizeDomain("//www.acme.io") === "acme.io", "scheme-relative URL");
    assert(normalizeDomain("acme.com") === "acme.com", "bare domain unchanged");
    expectCode("invalid_domain", () => { normalizeDomain("dana@acme.com"); });
    expectCode("invalid_domain", () => { normalizeDomain("https://user:pw@acme.com/"); });
    for (const bad of ["", "acme", "www.", "ac me.com", "*.acme.com", "acme..com", "-acme.com"]) {
        expectCode("invalid_domain", () => { normalizeDomain(bad); });
    }
    // Attio would store a Unicode domain in another spelling than a rule compares,
    // so only ASCII (punycode for internationalized names) is accepted.
    for (const unicode of ["bücher.de", "аcme.com", "acme.ｃom"]) {
        expectCode("invalid_domain", () => { normalizeDomain(unicode); });
    }
    assert(normalizeDomain("xn--bcher-kva.de") === "xn--bcher-kva.de", "punycode accepted");

    label("slugs and record ids are normalized, and UUID object ids refused");
    assert(normalizeSlug(" Deals ", "object") === "deals", "slug lowercased");
    assert(normalizeSlug("associated_company", "attribute") === "associated_company", "underscores allowed");
    expectCode("invalid_slug", () => { normalizeSlug("97052eb9-e65e-443f-a297-f2d9a4a7f795", "object"); });
    expectCode("invalid_slug", () => { normalizeSlug("deal stage", "attribute"); });
    expectCode("invalid_input", () => { normalizeSlug("  ", "object"); });
    assert(normalizeRecordId(" 8A1B2C3D-1111-4222-8333-944455556666 ", "recordId") === RECORD, "record id lowercased");
    expectCode("invalid_record_id", () => { normalizeRecordId("123", "recordId"); });
    expectCode("invalid_record_id", () => { normalizeRecordId("8a1b2c3d-1111-4222-8333-94445555666g", "recordId"); });

    label("typed values become Attio's JSON");
    const text = fields("text");
    text.text = "Acme deal";
    assert(JSON.stringify(prepareValue("name", text).json) === "{\"value\":\"Acme deal\"}", "text");
    const amount = fields("currency");
    amount.amount = 1200.5;
    assert(JSON.stringify(prepareValue("value", amount).json) === "{\"currency_value\":1200.5}", "currency");
    const number = fields("number");
    number.number = 3;
    assert(prepareValue("score", number).kind === "number", "number kind");
    const box = fields("checkbox");
    box.checked = false;
    assert(JSON.stringify(prepareValue("hot", box).json) === "{\"value\":false}", "checkbox");
    const day = fields("date");
    day.date = "2026-10-26";
    assert(JSON.stringify(prepareValue("close_date", day).json) === "{\"value\":\"2026-10-26\"}", "date");
    const option = fields("select");
    option.option = "Inbound";
    assert(JSON.stringify(prepareValue("source", option).json) === "{\"option\":\"Inbound\"}", "select");
    const mail = fields("email");
    mail.email = " Dana@Acme.com";
    assert(JSON.stringify(prepareValue("email_addresses", mail).json) === "{\"email_address\":\"dana@acme.com\"}", "email normalized");
    const site = fields("domain");
    site.domain = "https://www.acme.com/";
    assert(JSON.stringify(prepareValue("domains", site).json) === "{\"domain\":\"acme.com\"}", "domain normalized");
    const link = fields("record");
    link.targetObject = "Companies";
    link.targetRecordId = COMPANY.toUpperCase();
    assert(
        JSON.stringify(prepareValue("associated_company", link).json) ===
            "{\"target_object\":\"companies\",\"target_record_id\":\"" + COMPANY + "\"}",
        "record reference normalized",
    );
    const member = fields("member");
    member.email = "Ehud@Example.com";
    assert(
        JSON.stringify(prepareValue("owner", member).json) === "{\"workspace_member_email_address\":\"ehud@example.com\"}",
        "member by email",
    );

    label("a value must carry exactly the field its type names");
    expectCode("invalid_value", () => { prepareValue("name", fields("text")); });
    const extra = fields("text");
    extra.text = "x";
    extra.number = 1;
    expectCode("invalid_value", () => { prepareValue("name", extra); });
    const half = fields("record");
    half.targetObject = "companies";
    expectCode("invalid_value", () => { prepareValue("associated_company", half); });
    expectCode("invalid_value", () => { prepareValue("name", fields("status")); });
    for (const bad of ["26/10/2026", "2026-02-31", "2026-02-29", "1900-02-29", "2026-04-31", "2026-13-01", "2026-00-10"]) {
        const badDay = fields("date");
        badDay.date = bad;
        expectCode("invalid_value", () => { prepareValue("close_date", badDay); });
    }
    for (const good of ["2024-02-29", "2000-02-29", "2026-12-31"]) {
        const goodDay = fields("date");
        goodDay.date = good;
        assert(prepareValue("close_date", goodDay).kind === "date", good + " is a real date");
    }

    label("a personal name needs a part, and fills all three Attio fields");
    const name = fields("personal-name");
    name.firstName = "Dana";
    name.lastName = "Lee";
    assert(JSON.stringify(prepareValue("name", name).json) === "{\"first_name\":\"Dana\",\"full_name\":\"Dana Lee\",\"last_name\":\"Lee\"}", "first and last");
    const full = fields("personal-name");
    full.fullName = "Dana";
    assert(JSON.stringify(prepareValue("name", full).json) === "{\"first_name\":\"\",\"full_name\":\"Dana\",\"last_name\":\"\"}", "full name only");
    expectCode("invalid_value", () => { prepareValue("name", fields("personal-name")); });
    const blank = fields("personal-name");
    blank.firstName = " ";
    expectCode("invalid_input", () => { prepareValue("name", blank); });
    const mixed = fields("personal-name");
    mixed.firstName = "Dana";
    mixed.text = "Dana";
    expectCode("invalid_value", () => { prepareValue("name", mixed); });
    const infinite = fields("number");
    infinite.number = 1 / 0;
    expectCode("invalid_value", () => { prepareValue("score", infinite); });

    label("values group by attribute, sorted, keeping input order within one");
    const pending: PendingValue[] = [
        { slug: "name", value: prepareValue("name", text) },
        { slug: "associated_people", value: { kind: "record", json: { target_object: "people", target_record_id: "a" } } },
        { slug: "associated_people", value: { kind: "record", json: { target_object: "people", target_record_id: "b" } } },
    ];
    const grouped = groupValues(pending);
    assert(grouped.length === 2 && grouped[0].slug === "associated_people" && grouped[1].slug === "name", "sorted groups");
    assert(grouped[0].values.length === 2, "multi-value kept together");
    assert(
        JSON.stringify(buildValues(grouped)) ===
            "{\"associated_people\":[{\"target_object\":\"people\",\"target_record_id\":\"a\"},{\"target_object\":\"people\",\"target_record_id\":\"b\"}],\"name\":[{\"value\":\"Acme deal\"}]}",
        "values body",
    );
    assert(JSON.stringify(uniqueSlugs(["name", "associated_people", "name"])) === "[\"associated_people\",\"name\"]", "unique sorted slugs");

    label("plain writes refuse status, unknown, read-only, extra and mismatched values");
    const definitions: Attribute[] = [
        definition("name", "text", false, true),
        definition("stage", "status", false, true),
        definition("created_at", "timestamp", false, false),
        definition("associated_people", "record-reference", true, true),
    ];
    requirePlainAttributes("deals", grouped, definitions, "use setStatus");
    const stage = fields("select");
    stage.option = "Won";
    expectCode("status_attribute", () => {
        requirePlainAttributes("deals", groupValues([{ slug: "stage", value: prepareValue("stage", stage) }]), definitions, "use setStatus");
    });
    expectCode("unknown_attribute", () => {
        requirePlainAttributes("deals", groupValues([{ slug: "nickname", value: prepareValue("nickname", text) }]), definitions, "");
    });
    expectCode("read_only_attribute", () => {
        requirePlainAttributes("deals", groupValues([{ slug: "created_at", value: prepareValue("created_at", day) }]), definitions, "");
    });
    expectCode("invalid_value", () => {
        const twice: PendingValue[] = [
            { slug: "name", value: prepareValue("name", text) },
            { slug: "name", value: prepareValue("name", text) },
        ];
        requirePlainAttributes("deals", groupValues(twice), definitions, "");
    });
    expectCode("invalid_value", () => {
        requirePlainAttributes("deals", groupValues([{ slug: "name", value: prepareValue("name", number) }]), definitions, "");
    });

    label("query bodies: shorthand equality, operators, fields, references, AND and sorts");
    const window = queryWindow(undefined, undefined);
    assert(JSON.stringify(buildQueryBody([], [], window)) === "{\"limit\":50,\"offset\":0}", "defaults only");
    const stageIs = prepareCondition({ attribute: "Stage", operator: "eq", value: "In Progress" });
    assert(
        JSON.stringify(buildQueryBody([stageIs], [], window)) === "{\"filter\":{\"stage\":\"In Progress\"},\"limit\":50,\"offset\":0}",
        "status equality by title",
    );
    const conditions: PreparedCondition[] = [
        prepareCondition({ attribute: "name", operator: "contains", value: "Acme" }),
        prepareCondition({ attribute: "email_addresses", operator: "eq", value: "acme.com", field: "email_domain" }),
        prepareCondition({ attribute: "associated_company", operator: "eq", value: { targetObject: "companies", targetRecordId: COMPANY } }),
        prepareCondition({ attribute: "value", operator: "gte", value: 1000 }),
    ];
    const body = buildQueryBody(conditions, [prepareSort({ attribute: "name", direction: "asc", field: "last_name" })], queryWindow(500, 1000));
    assert(
        JSON.stringify(body) ===
            "{\"filter\":{\"$and\":[{\"name\":{\"$contains\":\"Acme\"}},{\"email_addresses\":{\"email_domain\":{\"$eq\":\"acme.com\"}}}," +
            "{\"associated_company\":{\"target_object\":\"companies\",\"target_record_id\":\"" + COMPANY + "\"}},{\"value\":{\"$gte\":1000}}]}," +
            "\"limit\":500,\"offset\":1000,\"sorts\":[{\"attribute\":\"name\",\"direction\":\"asc\",\"field\":\"last_name\"}]}",
        "combined query body: " + JSON.stringify(body),
    );
    assert(
        JSON.stringify(buildQueryBody([prepareCondition({ attribute: "owner", operator: "eq", value: { memberId: COMPANY.toUpperCase() } })], [], window)) ===
            "{\"filter\":{\"owner\":{\"referenced_actor_id\":\"" + COMPANY + "\",\"referenced_actor_type\":\"workspace-member\"}},\"limit\":50,\"offset\":0}",
        "member filter",
    );
    expectCode("invalid_filter", () => { prepareCondition({ attribute: "owner", operator: "contains", value: { memberId: COMPANY } }); });
    expectCode("invalid_record_id", () => { prepareCondition({ attribute: "owner", operator: "eq", value: { memberId: "ehud@example.com" } }); });
    expectCode("invalid_filter", () => { prepareCondition({ attribute: "name", operator: "contains", value: 3 }); });
    expectCode("invalid_filter", () => { prepareCondition({ attribute: "value", operator: "gt", value: true }); });
    expectCode("invalid_filter", () => {
        prepareCondition({ attribute: "associated_company", operator: "contains", value: { targetObject: "companies", targetRecordId: COMPANY } });
    });
    expectCode("invalid_record_id", () => {
        prepareCondition({ attribute: "associated_company", operator: "eq", value: { targetObject: "companies", targetRecordId: "acme" } });
    });
    expectCode("invalid_slug", () => { prepareCondition({ attribute: "a-b", operator: "eq", value: "x" }); });
    expectCode("invalid_page_size", () => { queryWindow(0, undefined); });
    expectCode("invalid_page_size", () => { queryWindow(501, undefined); });
    expectCode("invalid_page_size", () => { queryWindow(2.5, undefined); });
    expectCode("invalid_offset", () => { queryWindow(10, -1); });
    expectCode("invalid_offset", () => { queryWindow(10, 1 / 0); });
    expectCode("invalid_offset", () => { noteWindow(undefined, 1e21); });
    expectCode("invalid_page_size", () => { queryWindow(0 / 0, undefined); });
    expectCode("invalid_page_size", () => { noteWindow(51, undefined); });
    assert(buildNotesQuery("deals", RECORD, noteWindow(undefined, 50)) === "?parent_object=deals&parent_record_id=" + RECORD + "&limit=50&offset=50", "notes query");

    label("upsert bodies hold the identifying value and only the given fields");
    assert(
        JSON.stringify(buildPersonValues("dana@acme.com", undefined, undefined, undefined, undefined, undefined, undefined)) ===
            "{\"email_addresses\":[{\"email_address\":\"dana@acme.com\"}]}",
        "email only",
    );
    assert(
        JSON.stringify(buildPersonValues("dana@acme.com", "Dana", undefined, undefined, "CTO", undefined, COMPANY)) ===
            "{\"company\":[{\"target_object\":\"companies\",\"target_record_id\":\"" + COMPANY + "\"}],\"email_addresses\":[{\"email_address\":\"dana@acme.com\"}]," +
            "\"job_title\":[{\"value\":\"CTO\"}],\"name\":[{\"first_name\":\"Dana\",\"full_name\":\"Dana\",\"last_name\":\"\"}]}",
        "name, title and company",
    );
    expectCode("invalid_input", () => { buildPersonValues("dana@acme.com", " ", undefined, undefined, undefined, undefined, undefined); });
    expectCode("invalid_input", () => { buildPersonValues("dana@acme.com", undefined, undefined, undefined, "", undefined, undefined); });
    expectCode("invalid_input", () => { buildPersonValues("dana@acme.com", undefined, undefined, undefined, undefined, " ", undefined); });
    expectCode("invalid_input", () => { buildCompanyValues("acme.com", undefined, " "); });
    assert(JSON.stringify(buildCompanyValues("acme.com", undefined, undefined)) === "{\"domains\":[{\"domain\":\"acme.com\"}]}", "domain only");
    assert(
        JSON.stringify(buildCompanyValues("acme.com", "Acme", "Rockets")) ===
            "{\"description\":[{\"value\":\"Rockets\"}],\"domains\":[{\"domain\":\"acme.com\"}],\"name\":[{\"value\":\"Acme\"}]}",
        "name and description",
    );

    label("records map to readable values");
    const raw = (JSON.parse(
        "{\"data\":{\"id\":{\"workspace_id\":\"w\",\"object_id\":\"o\",\"record_id\":\"" + RECORD + "\"},\"created_at\":\"2026-10-26T10:00:00.000000000Z\"," +
        "\"web_url\":\"https://app.attio.com/x/deals/" + RECORD + "\",\"values\":{" +
        "\"name\":[{\"active_from\":\"t\",\"active_until\":null,\"attribute_type\":\"text\",\"value\":\"Acme pilot\"}]," +
        "\"stage\":[{\"attribute_type\":\"status\",\"status\":{\"id\":{\"status_id\":\"s-lead\"},\"title\":\"Lead\",\"is_archived\":false}}]," +
        "\"value\":[{\"attribute_type\":\"currency\",\"currency_value\":5000,\"currency_code\":\"USD\"}]," +
        "\"owner\":[{\"attribute_type\":\"actor-reference\",\"referenced_actor_type\":\"workspace-member\",\"referenced_actor_id\":\"m-1\"}]," +
        "\"created_by\":[{\"attribute_type\":\"actor-reference\",\"referenced_actor_type\":\"api-token\",\"referenced_actor_id\":null}]," +
        "\"associated_company\":[{\"attribute_type\":\"record-reference\",\"target_object\":\"companies\",\"target_record_id\":\"" + COMPANY + "\"}]," +
        "\"source\":[{\"attribute_type\":\"select\",\"option\":{\"id\":{\"option_id\":\"o-1\"},\"title\":\"Inbound\"}}]," +
        "\"emails\":[{\"attribute_type\":\"email-address\",\"email_address\":\"dana@acme.com\",\"original_email_address\":\"Dana@acme.com\"}]," +
        "\"domains\":[{\"attribute_type\":\"domain\",\"domain\":\"acme.com\",\"root_domain\":\"acme.com\"}]," +
        "\"person\":[{\"attribute_type\":\"personal-name\",\"first_name\":\"Dana\",\"last_name\":\"Lee\",\"full_name\":\"Dana Lee\"}]," +
        "\"hot\":[{\"attribute_type\":\"checkbox\",\"value\":true}]," +
        "\"score\":[{\"attribute_type\":\"number\",\"value\":4}]," +
        "\"close_date\":[{\"attribute_type\":\"date\",\"value\":\"2026-11-01\"}]," +
        "\"office\":[{\"attribute_type\":\"location\",\"line_1\":\"1 Main St\",\"line_2\":null,\"locality\":\"Springfield\",\"region\":null,\"postcode\":\"12345\",\"country_code\":\"US\"}]," +
        "\"phone_numbers\":[{\"attribute_type\":\"phone-number\",\"original_phone_number\":\"+15558675309\",\"normalized_phone_number\":\"+15558675309\",\"country_code\":\"US\"}]," +
        "\"description\":[]}}}",
    ) as RecordEnvelope).data;
    const record = recordFrom(raw, "deals");
    assert(record.object === "deals" && record.recordId === RECORD, "identity");
    assert(record.webUrl.endsWith(RECORD) && record.createdAt.startsWith("2026-10-26"), "link and time");
    assert(first(record, "name").value === "Acme pilot", "text");
    assert(first(record, "stage").value === "Lead" && first(record, "stage").optionId === "s-lead", "status title and id");
    assert(first(record, "value").value === "5000 USD" && first(record, "value").number === 5000, "currency");
    assert(first(record, "owner").value === "m-1" && first(record, "owner").actorId === "m-1", "owner member id");
    assert(first(record, "owner").actorType === "workspace-member", "owner actor type");
    assert(first(record, "created_by").value === "api-token", "non-member actor");
    assert(first(record, "associated_company").targetRecordId === COMPANY && first(record, "associated_company").value === COMPANY, "reference");
    assert(first(record, "associated_company").targetObject === "companies", "reference object");
    assert(first(record, "source").value === "Inbound", "select title");
    assert(first(record, "emails").value === "dana@acme.com", "email");
    assert(first(record, "domains").value === "acme.com", "domain");
    assert(first(record, "person").value === "Dana Lee", "personal name");
    assert(first(record, "hot").value === "true", "checkbox");
    assert(first(record, "score").number === 4 && first(record, "score").value === "4", "number");
    assert(first(record, "close_date").value === "2026-11-01", "date");
    assert(first(record, "office").value === "1 Main St, Springfield, 12345, US", "location");
    assert(first(record, "phone_numbers").value === "+15558675309", "phone");
    assert(record.values["description"] === undefined, "empty attributes left out");
    assert(currentStatus(raw, "stage").title === "Lead", "current status");
    assert(currentStatus(raw, "name").title === "", "no status");

    label("statuses resolve by id or case-insensitive title, and unknown ones list the valid titles");
    const statuses: Status[] = [
        { statusId: "s-lead", title: "Lead", celebrationEnabled: false },
        { statusId: "s-won", title: "Won 🎉", celebrationEnabled: true },
    ];
    assert(findStatus(statuses, " won 🎉 ", "deals", "stage").statusId === "s-won", "title, case-insensitive");
    assert(findStatus(statuses, "S-LEAD", "deals", "stage").title === "Lead", "status id");
    let listed = "";
    try {
        findStatus(statuses, "Closed", "deals", "stage");
    } catch (cause) {
        if (cause instanceof AttioError && cause.code === "unknown_status" && cause.status === 0) listed = cause.message;
    }
    assert(listed.includes("\"Lead\"") && listed.includes("\"Won 🎉\""), "valid titles listed: " + listed);

    label("objects, attributes and notes map to compact types");
    const objects = JSON.parse(
        "{\"data\":[{\"id\":{\"workspace_id\":\"w\",\"object_id\":\"o-deals\"},\"api_slug\":\"deals\",\"singular_noun\":\"Deal\",\"plural_noun\":\"Deals\",\"created_at\":\"t\"}," +
        "{\"id\":{\"workspace_id\":\"w\",\"object_id\":\"o-x\"},\"api_slug\":null,\"singular_noun\":null,\"plural_noun\":null,\"created_at\":\"t\"}]}",
    ) as ObjectList;
    assert(objectFrom(objects.data[0]).slug === "deals" && objectFrom(objects.data[0]).pluralNoun === "Deals", "object");
    assert(objectFrom(objects.data[1]).slug === "", "null slug");
    const attributes = JSON.parse(
        "{\"data\":[{\"id\":{\"attribute_id\":\"a-1\"},\"api_slug\":\"value\",\"title\":\"Value\",\"description\":null,\"type\":\"currency\"," +
        "\"is_multiselect\":false,\"is_writable\":true,\"is_required\":false,\"is_unique\":false,\"is_archived\":false," +
        "\"config\":{\"currency\":{\"default_currency_code\":\"EUR\",\"display_type\":\"symbol\"},\"record_reference\":null}}]}",
    ) as AttributeList;
    const value = attributeFrom(attributes.data[0]);
    assert(value.slug === "value" && value.type === "currency" && value.currencyCode === "EUR" && value.description === "", "attribute");
    const note = noteFrom((JSON.parse(
        "{\"data\":{\"id\":{\"workspace_id\":\"w\",\"note_id\":\"n-1\"},\"parent_object\":\"deals\",\"parent_record_id\":\"" + RECORD + "\"," +
        "\"title\":\"Met at meetup\",\"meeting_id\":null,\"content_plaintext\":\"Wants a demo\",\"content_markdown\":\"Wants a **demo**\",\"tags\":[]," +
        "\"created_by_actor\":{\"type\":\"api-token\",\"id\":null},\"created_at\":\"t\"}}",
    ) as NoteEnvelope).data);
    assert(note.noteId === "n-1" && note.text === "Wants a demo" && note.markdown === "Wants a **demo**", "note");

    label("errors keep Attio's code and message, and mark retryable failures");
    const notFound = failureFrom(404, "Not Found", "{\"status_code\":404,\"type\":\"invalid_request_error\",\"code\":\"not_found\",\"message\":\"Record not found.\"}");
    assert(notFound.code === "not_found" && notFound.type === "invalid_request_error" && notFound.message.includes("Record not found."), "body code");
    assert(!notFound.retryable, "404 is final");
    assert(failureFrom(429, "Too Many Requests", "{\"code\":\"rate_limit_exceeded\",\"message\":\"slow down\"}").retryable, "429 retryable");
    assert(failureFrom(409, "Conflict", "{\"code\":\"concurrent_write_conflict\",\"message\":\"retry\"}").retryable, "write conflict retryable");
    assert(!failureFrom(409, "Conflict", "{\"code\":\"uniqueness_conflict\",\"message\":\"dup\"}").retryable, "uniqueness conflict is final");
    const fallback = failureFrom(502, "Bad Gateway", "<html>");
    assert(fallback.code === "server_error" && fallback.message === "Attio request failed: HTTP 502 Bad Gateway", "non-JSON body");
    assert(failureFrom(403, "Forbidden", "{malformed").code === "forbidden", "malformed body");

    label("Retry-After becomes seconds to wait, from a number or an HTTP date");
    const now = 1684852861000;
    assert(retryAfterSeconds("120", now) === 120, "delta seconds");
    assert(retryAfterSeconds("Tue, 23 May 2023 14:42:01 GMT", now) === 60, "HTTP date a minute ahead");
    assert(retryAfterSeconds("Tue, 23 May 2023 14:40:01 GMT", now) === 0, "a past date means no wait");
    assert(retryAfterSeconds("Thu, 29 Feb 2024 00:00:00 GMT", 1709164800000) === 0, "leap day");
    assert(retryAfterSeconds("", now) === null, "absent");
    assert(retryAfterSeconds("soon", now) === null, "malformed");
    assert(retryAfterSeconds("Tue, 23 Foo 2023 14:42:01 GMT", now) === null, "unknown month");
    assert(retryAfterSeconds("Tue, 23 May 2023 14:42 GMT", now) === null, "short time");
    assert(retryAfterSeconds("99999999999999999999", now) === null, "seconds beyond a safe integer");
    assert(retryAfterSeconds("9".repeat(400), now) === null, "a 400-digit number");
    assert(retryAfterSeconds("Tue, 99 May 2023 14:42:01 GMT", now) === null, "day out of range");
    assert(retryAfterSeconds("Tue, 0 May 2023 14:42:01 GMT", now) === null, "day zero");
    assert(retryAfterSeconds("Tue, 23 May 2023 99:99:99 GMT", now) === null, "time out of range");
    assert(retryAfterSeconds("Tue, 23 May 2023 24:00:00 GMT", now) === null, "hour 24");
    assert(retryAfterSeconds("Tue, 23 May 2023 14:60:01 GMT", now) === null, "minute 60");
    assert(retryAfterSeconds("Xyz, 23 May 2023 14:42:01 GMT", now) === null, "unknown weekday");
    assert(retryAfterSeconds("Tue, 23 May 23 14:42:01 GMT", now) === null, "two-digit year");
    assert(retryAfterSeconds("Tue, 23 May 02023 14:42:01 GMT", now) === null, "five-digit year");

    label("pages end on a short or empty page");
    const fullPage = pageOf(["a", "b"], 0, 2, 2);
    assert(fullPage.nextOffset === 2 && !fullPage.isComplete && fullPage.items.length === 2, "full page continues");
    const short = pageOf(["a"], 50, 50, 7);
    assert(short.nextOffset === 57 && short.isComplete, "short page completes, and its next offset is past the end");
    const emptyPage = pageOf<string>([], 2, 2, 0);
    assert(emptyPage.nextOffset === 2 && emptyPage.isComplete, "an empty page completes and stays at the end");
    const filtered = pageOf<string>([], 0, 2, 2);
    assert(filtered.nextOffset === 2 && !filtered.isComplete, "paging counts what Attio returned, not what was kept");

    label("operations validate input before any check or request");
    expectCode("invalid_slug", () => { listAttributes("97052eb9-e65e-443f-a297-f2d9a4a7f795"); });
    expectCode("invalid_slug", () => { listStatuses("deals", "deal-stage"); });
    expectCode("invalid_record_id", () => { getRecord("deals", "acme"); });
    expectCode("invalid_page_size", () => { queryRecords("deals", { limit: 501 }); });
    expectCode("invalid_filter", () => { queryRecords("deals", { filter: [{ attribute: "name", operator: "contains", value: 1 }] }); });
    expectCode("invalid_email", () => { findPersonByEmail("dana"); });
    expectCode("invalid_email", () => { upsertPerson({ email: "dana at acme" }); });
    expectCode("invalid_domain", () => { upsertCompany({ domain: "acme" }); });
    expectCode("invalid_value", () => { createRecord("deals", { values: [{ attribute: "name", type: "text" }] }); });
    expectCode("invalid_input", () => { createRecord("deals", { status: { attribute: "stage", title: " " } }); });
    expectCode("invalid_record_id", () => { updateRecord("deals", "nope", [{ attribute: "name", type: "text", text: "x" }]); });
    expectCode("invalid_value", () => { updateRecord("deals", RECORD, [{ attribute: "value", type: "currency", text: "5000" }]); });
    expectCode("invalid_input", () => { setStatus("deals", RECORD, "stage", "  "); });
    expectCode("invalid_record_id", () => { setStatus("deals", "x", "stage", "Won"); });
    expectCode("invalid_input", () => { createNote("deals", RECORD, { title: " ", content: "x" }); });
    expectCode("invalid_page_size", () => { listNotes("deals", RECORD, { limit: 51 }); });
    expectCode("invalid_offset", () => { queryRecords("deals", { offset: 1 / 0 }); });
    expectCode("invalid_offset", () => { listNotes("deals", RECORD, { offset: 1e21 }); });
    expectCode("invalid_input", () => { upsertPerson({ email: "dana@acme.com", jobTitle: "" }); });

    if (secrets.get("ATTIO_API_KEY") !== undefined) {
        label("skip: valid input reaches the token read only when ATTIO_API_KEY is not bound");
        return;
    }
    label("an unbound API key is reported before any request");
    expectCode("missing_token", () => { listObjects(); });
    expectCode("missing_token", () => { listMembers(); });
    expectCode("missing_token", () => { listAttributes("deals"); });
    expectCode("missing_token", () => { getRecord("deals", RECORD); });
    expectCode("missing_token", () => { queryRecords("deals", { filter: [{ attribute: "stage", operator: "eq", value: "Lead" }] }); });
    expectCode("missing_token", () => { findPersonByEmail("Dana@Acme.com"); });
    expectCode("missing_token", () => { upsertPerson({ email: "dana@acme.com", firstName: "Dana" }); });
    expectCode("missing_token", () => { upsertCompany({ domain: "https://www.acme.com", name: "Acme" }); });
    expectCode("missing_token", () => { createRecord("deals", { values: [{ attribute: "name", type: "text", text: "Acme" }] }); });
    expectCode("missing_token", () => { createRecord("deals", { status: { attribute: "stage", title: "Lead" } }); });
    expectCode("missing_token", () => { updateRecord("deals", RECORD, [{ attribute: "name", type: "text", text: "Acme" }]); });
    expectCode("missing_token", () => { setStatus("deals", RECORD, "stage", "Won"); });
    expectCode("missing_token", () => { createNote("deals", RECORD, { title: "Met", content: "Wants a demo" }); });
    expectCode("missing_token", () => { listNotes("deals", RECORD); });
}
