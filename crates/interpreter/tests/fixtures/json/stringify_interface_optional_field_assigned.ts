// An interface-typed object literal built empty (or partial) and then filled by
// field assignment must serialize by each field's *declared* type. The null-fill
// for an absent optional once leaked `Type::Null` into the collected shape, so
// JSON.stringify rendered every later-assigned field as `null`.

interface Inner {
    eq: string;
}

interface Filter {
    team?: Inner;
    name?: string;
    count?: number;
}

function buildTeam(id: string): Filter {
    const out: Filter = {};
    out.team = { eq: id };
    return out;
}

function main(): void {
    assert(
        JSON.stringify(buildTeam("T1")) === "{\"team\":{\"eq\":\"T1\"}}",
        "object-typed optional field assigned after empty literal",
    );

    const mixed: Filter = {};
    mixed.name = "hi";
    mixed.count = 7;
    assert(
        JSON.stringify(mixed) === "{\"count\":7,\"name\":\"hi\"}",
        "primitive optional fields assigned after empty literal",
    );

    const empty: Filter = {};
    assert(JSON.stringify(empty) === "{}", "all-absent optionals still omitted");
}
