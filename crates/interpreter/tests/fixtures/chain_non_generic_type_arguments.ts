// expect-error: expects 0 type argument(s), got 1
// expect-error: non-generic call expects 0 type arguments
import { Response } from "submilli:http";
function method(r: Response | null): void { r?.toString<number>(); }
function property(r: { call: () => number } | null): void { r?.call<number>(); }
function main(): void { method(null); property(null); }

interface Callable { (): number; }
function callable(r: { call: Callable } | null): void { r?.call<number>(); }
function union(r: { call: (() => number) | (() => string) } | null): void { r?.call<number>(); }
