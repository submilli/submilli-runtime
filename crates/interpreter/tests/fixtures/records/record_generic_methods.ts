interface Identity { [key: string]: unknown; id<T>(value: T): T; }
interface Renamed extends Identity { id<U>(value: U): U; }
interface Concrete { id(value: number): number; }
interface GenericOverride extends Concrete { id<T>(value: T): T; }
interface IndexedFactory { [key: string]: () => number; read<T>(): T; }
interface OtherIdentity { id<U>(value: U): U; }
interface SameBases extends Identity, OtherIdentity {}
function use(value: Renamed): string { return value.id<string>("ok"); }
function main(): void {}
