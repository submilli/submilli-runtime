// No ordinary index read: postfix must register its own bounds-error message.
function main(): void {
  const bytes = new Uint8Array([1]);
  const old = bytes[0]++;
  assert(old === 1, "postfix-only byte program");
}
