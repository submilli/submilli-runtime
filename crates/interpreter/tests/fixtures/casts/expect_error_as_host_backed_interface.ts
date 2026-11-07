// A Response retains host-owned behavior and cannot be manufactured by a cast.
// expect-error-count: 6
// expect-error: `as` can't produce a `Response`
// expect-error: `as` can't produce a `Response | null`
// expect-error: `as` can't produce a `{ s?: Response }`
// expect-error: `as` can't produce a `W`
// expect-error: `as` can't produce a `{ [key: string]: Response }`
// expect-error: `session.get<Response>` cannot be verified at runtime
import * as http from "submilli:http";
import * as session from "submilli:session";
interface W { s: http.Response }
function main(): void {
 const text = "{}";
 const parsed = JSON.parse(text) as http.Response;
 const nullable = JSON.parse(text) as http.Response | null;
 const optional = JSON.parse(text) as { s?: http.Response };
 const wrapped = JSON.parse(text) as W;
 const record = JSON.parse(text) as { [k: string]: http.Response };
 const stored = session.get<http.Response>("k");
}
