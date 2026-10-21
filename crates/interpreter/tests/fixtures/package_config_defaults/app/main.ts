// deny-capability: http.get
import { endpoint, timeout, active, zero, fetchText } from "@test/config";
function main(): void {
    assert(endpoint() === "https://default.test");
    assert(timeout() === -5);
    assert(active());
    assert(1 / zero() < 0);
    let denied = false;
    try { fetchText(); } catch (error: PermissionDeniedError) { denied = true; }
    assert(denied, "configuration grants no HTTP permission");
}
