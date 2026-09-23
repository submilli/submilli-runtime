function text(s: string): string { return s; }
function main(): void {
 let calls = 0;
 const rhs = (): string => { calls += 1; return "y"; };
 const a = text("x") && rhs();
 const b = text("") && rhs();
 const c = text("x") || rhs();
 const d = text("") || rhs();
 assert(`${a}/${b}/${c}/${d}` === "y//x/y", "logical values");
 assert(calls === 2, "short circuit");
}
