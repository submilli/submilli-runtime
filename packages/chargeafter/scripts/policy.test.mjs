import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const bin = process.env.SUBMILLI_BIN ?? "/private/tmp/submilli-0.5.1-aarch64-apple-darwin";
const cwd = new URL("../../..", import.meta.url).pathname;
function run(args, env) { return spawnSync(bin, args, { cwd, env: { ...process.env, ...env }, encoding: "utf8" }); }

test("real and demo packages enforce the same caller policy matrix in an isolated local store", async () => {
    const home = await mkdtemp(join(tmpdir(), "chargeafter-policy-"));
    try {
        const env = { SUBMILLI_HOME: home };
        for (const [pkg, extra] of [["@submilli/chargeafter", ["--config", "baseUrl=https://api-sandbox.ca-dev.co"]], ["@submilli/chargeafter-demo", []]]) {
            const published = run(["build", "publish-local", "--deny-warnings", "-p", pkg, ...extra], env);
            assert.equal(published.status, 0, published.stderr);
        }
        const blueprint = join(home, "policy.yaml");
        await writeFile(blueprint, `kind: blueprint
name: chargeafter-policy
packages:
  - '@submilli/chargeafter-demo'
variables:
  storeId:
    required: true
default: deny
permissions:
  main:
    - capability: chargeafter.com/links.sendApply
      filter: storeId == \${vars.storeId} and deliveryMethod == "Email"
      action: allow
    - capability: chargeafter.com/charges.get
      action: allow
    - capability: chargeafter.com/charges.settle
      action: deny
  '@submilli/chargeafter-demo': []
`);
        const script = join(home, "demo.ts");
        await writeFile(script, `import { getCharge, sendApplyLink, settleCharge } from "@submilli/chargeafter-demo";
import { get } from "submilli:http";
function denied(capability: string, action: () => void): void { try { action(); } catch (error) { assert(error.name === "PermissionDeniedError" && error.message.includes(capability), "wrong denial"); return; } assert(false, "expected denial"); }
function main(): string { denied("http.get", () => { get("https://example.invalid/"); }); denied("chargeafter.com/links.sendApply", () => { sendApplyLink({ consumer: { firstName: "Ada", lastName: "Morris", email: "ada@example.test" }, deliveryMethod: "Email", storeId: "hooli", channel: "E_COMMERCE" }); }); const before = getCharge("charge-authorized"); if (before === null) throw new Error("fixture missing"); const settled = before.settledAmount; denied("chargeafter.com/charges.settle", () => { settleCharge("charge-authorized", 10.5); }); const after = getCharge("charge-authorized"); assert(after !== null && after.settledAmount === settled, "denial mutated charge"); const link = sendApplyLink({ consumer: { firstName: "Ada", lastName: "Morris", email: "ada@example.test" }, deliveryMethod: "Email", storeId: "pied_piper", channel: "E_COMMERCE" }); assert(link.linkId.length > 0, "allowed link"); return "ok"; }`);
        const lintDemo = run(["blueprint", "lint", "--deny-warnings", blueprint], env);
        assert.equal(lintDemo.status, 0, lintDemo.stderr);
        const missingVariable = run(["run", "--blueprint", blueprint, script], env);
        assert.notEqual(missingVariable.status, 0, "required blueprint variable must reject an unbound run");
        const demo = run(["run", "--blueprint", blueprint, "--var", "storeId=pied_piper", script], env);
        assert.equal(demo.status, 0, demo.stderr);
        const realBlueprint = join(home, "real.yaml");
        const realPolicy = (await (await import("node:fs/promises")).readFile(blueprint, "utf8")).replaceAll("@submilli/chargeafter-demo", "@submilli/chargeafter").replace("  '@submilli/chargeafter': []", `  '@submilli/chargeafter':
    - capability: http.get
      filter: host == "api-sandbox.ca-dev.co"
      action: allow
    - capability: http.post
      filter: host == "api-sandbox.ca-dev.co"
      action: allow
    - capability: http.patch
      filter: host == "api-sandbox.ca-dev.co"
      action: allow
    - capability: secrets.get
      filter: name == "CHARGEAFTER_PRIVATE_KEY"
      action: allow
secrets:
  CHARGEAFTER_PRIVATE_KEY:
    store: chargeafter_test`);
        await writeFile(realBlueprint, realPolicy);
        const realScript = join(home, "real.ts");
        await writeFile(realScript, `import { sendApplyLink } from "@submilli/chargeafter";
function denied(capability: string, action: () => void): void { try { action(); } catch (error) { assert(error.name === "PermissionDeniedError" && error.message.includes(capability), "wrong denial"); return; } assert(false, "expected denial"); }
function main(): string { denied("chargeafter.com/links.sendApply", () => { sendApplyLink({ consumer: { firstName: "Ada", lastName: "Morris", email: "ada@example.test" }, deliveryMethod: "Email", storeId: "hooli", channel: "E_COMMERCE" }); }); try { sendApplyLink({ consumer: { firstName: "Ada", lastName: "Morris", email: "ada@example.test" }, deliveryMethod: "Email", storeId: "pied_piper", channel: "E_COMMERCE" }); } catch (error) { assert(error.name === "ChargeAfterError" && error.message.includes("CHARGEAFTER_PRIVATE_KEY"), "expected missing key before HTTP"); return "ok"; } assert(false, "expected missing key"); return "unreachable"; }`);
        const lintReal = run(["blueprint", "lint", "--deny-warnings", realBlueprint], env);
        assert.equal(lintReal.status, 0, lintReal.stderr);
        const real = run(["run", "--blueprint", realBlueprint, "--var", "storeId=pied_piper", realScript], env);
        assert.equal(real.status, 0, real.stderr);
    } finally { await rm(home, { recursive: true, force: true }); }
});
