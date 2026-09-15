// expect-error: the receiver can be `null`
class I { y: number = 2; }
class O { b: I | null = new I(); constructor(ok: boolean) { if(!ok) { this.b = null; } } }
function main(): number { let o: O | null = new O(true); if(o !== null && o.b !== null) { o = new O(false); return o.b.y; } return 0; }
