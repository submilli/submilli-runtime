// Written: exact optional property writes deliberately differ from Submilli's
// ordinary TypeScript optional-property policy; preserve that comparison.
// @strict: true
// @exactOptionalPropertyTypes: true
function acceptsOptional(value: { a?: number }): void {}
function acceptsExplicit(value: { a?: number | undefined }): void {}
acceptsOptional({});
acceptsOptional({ a: undefined });
acceptsExplicit({ a: undefined });
function main(): void {}
