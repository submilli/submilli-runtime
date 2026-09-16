// expect-error: cannot access `uuid` before its initialization
import uuid from "submilli:uuid"; class Fake { v4(): string { return "x"; } } function main(): void { uuid.v4(); const uuid = new Fake(); }
