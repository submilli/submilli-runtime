// A Response retains host-owned behavior and cannot be manufactured by a cast.
// expect-error-count: 5
// expect-error: `as` can't produce a `Response`
// expect-error: `as` can't produce a `Response | null`
// expect-error: `as` can't produce a `{ s?: Response }`
// expect-error: `as` can't produce a `W`
// expect-error: `as` can't produce a `{ [key: string]: Response }`
import * as http from "submilli:http";
interface W { s: http.Response }
function main(): void {
 const text = "{}";
 const parsed = JSON.parse(text) as http.Response;
 const nullable = JSON.parse(text) as http.Response | null;
 const optional = JSON.parse(text) as { s?: http.Response };
 const wrapped = JSON.parse(text) as W;
 const record = JSON.parse(text) as { [k: string]: http.Response };
}
