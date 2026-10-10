let callback: ((value: number) => number) | null = (value: number): number => value;
let calls = 0;
function clear(): boolean { callback = null; return false; }
function argument(): number { calls += 1; return 1; }
function main(): void {
 let caught = false;
 try { if (callback !== null && !clear()) callback(argument()); }
 catch (error) { caught = error instanceof TypeError; }
 assert(caught); assert(calls === 1);
}
