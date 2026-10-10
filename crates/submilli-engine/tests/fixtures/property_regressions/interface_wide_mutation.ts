interface Box { value: number | string | null; }
interface Dynamic { value: unknown; }
interface NonNull { value: number | string; }
function main(): void {
    const box: Box = { value: "first" };
    assert(JSON.stringify(box) === '{"value":"first"}', "mixed nullable initializer");
    box.value = 2;
    assert(JSON.stringify(box) === '{"value":2}', "mixed nullable write");
    box.value = null;
    assert(JSON.stringify(box) === '{"value":null}', "mixed nullable clear");
    const dynamic: Dynamic = { value: 1 };
    dynamic.value = "text";
    assert(JSON.stringify(dynamic) === '{"value":"text"}', "unknown write");
    const nonNull: NonNull = { value: 1 };
    nonNull.value = "text";
    assert(JSON.stringify(nonNull) === '{"value":"text"}', "non-null mixed union write");
}
