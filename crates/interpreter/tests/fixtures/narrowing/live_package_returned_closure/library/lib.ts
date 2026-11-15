// expect-error: expected `number`, got `number | null`
// A call in the condition invalidates the preceding module-variable guard.
let current: number | null = 3;
function clear(): boolean { current = null; return false; }
/** Return callback. */
export function factory(): () => number { return (): number => { if(current === null || clear()) return 0; return current; }; }
