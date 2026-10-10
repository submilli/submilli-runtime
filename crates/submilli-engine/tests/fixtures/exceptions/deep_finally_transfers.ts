let count = 0;
function nested(mask: number): string {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
try {
return "body";
} finally { count = count + 1; if (mask === 0) { return "r0"; } }
} finally { count = count + 1; if (mask === 1) { return "r1"; } }
} finally { count = count + 1; if (mask === 2) { return "r2"; } }
} finally { count = count + 1; if (mask === 3) { return "r3"; } }
} finally { count = count + 1; if (mask === 4) { return "r4"; } }
} finally { count = count + 1; if (mask === 5) { return "r5"; } }
} finally { count = count + 1; if (mask === 6) { return "r6"; } }
} finally { count = count + 1; if (mask === 7) { return "r7"; } }
} finally { count = count + 1; if (mask === 8) { return "r8"; } }
} finally { count = count + 1; if (mask === 9) { return "r9"; } }
} finally { count = count + 1; if (mask === 10) { return "r10"; } }
} finally { count = count + 1; if (mask === 11) { return "r11"; } }
} finally { count = count + 1; if (mask === 12) { return "r12"; } }
} finally { count = count + 1; if (mask === 13) { return "r13"; } }
} finally { count = count + 1; if (mask === 14) { return "r14"; } }
} finally { count = count + 1; if (mask === 15) { return "r15"; } }
} finally { count = count + 1; if (mask === 16) { return "r16"; } }
} finally { count = count + 1; if (mask === 17) { return "r17"; } }
} finally { count = count + 1; if (mask === 18) { return "r18"; } }
} finally { count = count + 1; if (mask === 19) { return "r19"; } }
}
function main(): void { assert(nested(-1) === "body"); assert(count === 20); count = 0; assert(nested(0) === "r0"); assert(count === 20); count = 0; assert(nested(19) === "r19"); assert(count === 20); }
