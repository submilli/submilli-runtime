function identity(value: Object): Object { return value; }
function direct(): Object { return { sum: 12 }; }
function union(): Object | { name: string } { return { sum: 12 }; }
function main(): void {
    assert(JSON.stringify(union()) === '{"sum":12}');
    const bound: Object = { sum: 12 };
    assert(JSON.stringify(direct()) === '{"sum":12}');
    assert(JSON.stringify(bound) === '{"sum":12}');
    assert(JSON.stringify(identity({ sum: 12 })) === '{"sum":12}');
}
