class Link { next: unknown = null; }
class Custom { toJson(): string { throw new TypeError("custom"); } }
function main(): void {
  const link = new Link();
  link.next = [link];
  let caught = 0;
  for (let i = 0; i < 100; i++) {
    try { JSON.stringify(link); } catch (error: RangeError) { caught++; }
    try { JSON.stringify(new Custom()); } catch (error: TypeError) { caught++; }
  }
  assert(caught === 200);
  assert(JSON.stringify({ ok: 1 }) === '{"ok":1}');
}
