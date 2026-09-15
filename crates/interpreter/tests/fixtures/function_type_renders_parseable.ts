// A rendered function type must carry a parameter name per position: the type
// grammar requires one, so a nameless rendering is text the reader cannot paste
// back. The renderings asserted here are pasted back — and run — in
// `function_type_rendering_round_trip.ts`.
// expect-error: got `(arg0: number, arg1: string) => string`
// expect-error: got `(arg0: number, ...arg1: number[]) => void`
// expect-error: got `((arg0: string) => number)[]`

function pick(a: number, b: string): string { return b + a.toString(); }
function collect(head: number, ...tail: number[]): void { assert(head + tail.length >= 0, "counted"); }

export function main(): string {
    const bad: number = pick;
    const badRest: number = collect;
    const badArray: number = [(s: string): number => s.length];
    return "x";
}
