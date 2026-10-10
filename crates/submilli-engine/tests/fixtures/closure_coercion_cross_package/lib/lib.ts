export function adapt(f: () => never): () => void { return f; }
export function roundtrip<T>(f: () => T): () => T { return f; }
export interface Sink<T> { emit(): T; }
export function drain<T>(sink: Sink<T>): T { return sink.emit(); }
