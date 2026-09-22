interface Dynamic { value: unknown; }
function main(): void {
    const high: Dynamic = { value: String.fromCharCode(0xd800) };
    const low: Dynamic = { value: String.fromCharCode(0xdc00) };
    const pair: Dynamic = { value: String.fromCharCode(0xd800, 0xdc00) };
    assert(JSON.stringify(high) === '{"value":"\\ud800"}', "lone high surrogate escapes");
    assert(JSON.stringify(low) === '{"value":"\\udc00"}', "lone low surrogate escapes");
    assert(JSON.stringify(pair) === '{"value":"' + String.fromCharCode(0xd800, 0xdc00) + '"}', "valid pair preserved");
}
