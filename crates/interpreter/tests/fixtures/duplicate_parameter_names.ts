// A repeated parameter name is rejected wherever a parameter list can appear.
// The later binding wins, so the earlier parameter is unreachable — the body can
// never name it, and the failure mode is a wrong value rather than an error,
// which is the worst shape of defect for this language's primary user.
//
// The shapes are what this file pins. That each is reported exactly *once* is a
// count, which no `expect-error` directive can express — it lives in
// `a_duplicate_parameter_reports_once_per_list` beside `resolve_params`.
// expect-error: duplicate parameter `a`
// expect-error: duplicate parameter `b`
// expect-error: duplicate parameter `c`
// expect-error: duplicate parameter `d`
// expect-error: duplicate parameter `e`
// expect-error: duplicate parameter `g`
// expect-error: duplicate parameter `h`
// expect-error: duplicate parameter `r`
// expect-error: duplicate parameter `v`
// A function *type* has no bindings — its names are documentary — but they still
// have to be distinct, and those lists reach neither `resolve_params` nor the
// arrow path.
// expect-error: duplicate parameter `w`
// expect-error: duplicate parameter `y`
// expect-error: previously declared here

interface Shaped {
    // Interface method signatures route through the same resolver.
    sig(g: number, g: number): number;
    // A function-typed *property* is an annotation, not a signature.
    cb: (y: number, y: number) => void;
}

class C {
    // A parameter property and a plain parameter of the same name collide too.
    constructor(private readonly b: number, b: number) {}

    method(c: number, c: number): number { return c; }

    static stat(d: number, d: number): number { return d; }
}

function dup(a: number, a: number): number { return a; }

// A rest parameter and a defaulted one are still parameters.
function withRest(r: number, ...r: number[]): number { return 0; }
function withDefault(v: number, v: number = 1): number { return v; }

// Three of a name: two duplicates, both anchored back at the first.
function three(h: number, h: number, h: number): number { return h; }

type FnAlias = (w: number, w: number) => number;

export function main(): string {
    // Arrows never reach `resolve_params`, so they need their own check.
    const arrow = (e: number, e: number): number => e;
    return dup(1, 2).toString();
}
