class Link { next: Link | null = null; value: number = 1; }
interface Chain { next: Chain | null; }
function main(): void {
  const a = new Link(); const b = new Link(); a.next = a; b.next = b;
  const c: Chain = { next: null }; c.next = c;
  let caught = 0;
  for (let i = 0; i < 70; i++) {
    try { console.log((a === b).toString()); } catch (e: RangeError) { caught++; }
    try { console.log(JSON.stringify(a)); } catch (e: RangeError) { caught++; }
    try { console.log(JSON.stringify(c)); } catch (e: RangeError) { caught++; }
    try { new Map<Link, number>().set(a, 1); } catch (e: RangeError) { caught++; }
  }
  assert(caught === 280);
  assert(new Link() === new Link());
  assert(JSON.stringify(new Link()) === '{"next":null,"value":1}');
}
