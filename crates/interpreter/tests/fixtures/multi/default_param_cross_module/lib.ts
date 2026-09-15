// Regression: omitting a trailing defaulted argument must typecheck across a
// module boundary. The default/arity info was dropped when a function was
// exported into another module's symbol table, so these calls reported
// "expected N argument(s)".

import { withReq, noArgs } from "./util";

export function call(): string {
    return withReq("u") + noArgs();
}
