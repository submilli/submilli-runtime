// expect-error: the receiver can be `null`
class Inner { n: number = 1; }
class Outer { value: Inner | null = new Inner(); }
function main(): void {
 const o = new Outer(); let i = 0;
 if(o.value !== null) { do { const n = o.value.n; o.value = null; i = i + 1; } while(i < 3); }
}
