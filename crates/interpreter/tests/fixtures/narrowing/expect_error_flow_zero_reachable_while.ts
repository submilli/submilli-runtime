// expect-error: the receiver can be `null`
function test(run: boolean): number {
 let x: string | null = null;
 if (run) { while (false) { return 1; } x = null; }
 else { x = "ok"; }
 return x.length;
}
function main(): void { console.log(test(true)); }
