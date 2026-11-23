// expect-error: generic call signatures are not yet supported
type Identity = <T>(value: T) => T;
interface GenericIdentity extends Identity {}

function main(): void {}
