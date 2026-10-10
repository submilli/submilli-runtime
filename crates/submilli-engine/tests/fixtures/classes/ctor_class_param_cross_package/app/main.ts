import {
  BigPayload,
  Box,
  Early,
  EitherHolder,
  Holder,
  Late,
  Marker,
  MaybeHolder,
  Payload,
  PayloadBox,
  PropHolder,
  TaggedHolder,
} from "@test/lib";

class LocalHolder extends Holder {
  constructor(p: Payload) {
    super(p, "local");
  }
  doubled(): number {
    return this.total() * 2;
  }
}

class Forwarder extends TaggedHolder {}

class LocalBox extends Box<string> {}

function main(): void {
  const h = new Holder(new Payload(3), "direct");
  assert(h.p.n === 3, "class-typed ctor param across the package boundary");
  assert(h.tag === "direct", "trailing primitive ctor param");
  assert(h.seen === 3, "ctor body reads the class-typed param at its own type");
  assert(h.total() === 3, "method on a cross-package instance");

  const sub = new Holder(new BigPayload(4), "subclass");
  assert(sub.total() === 40, "subclass instance into a class-typed ctor param");

  const t = new TaggedHolder(new Payload(7));
  assert(t.total() === 7, "imported subclass forwarding a class-typed super() arg");
  assert(t.tag === "tagged", "imported subclass's own super() literal");

  const l = new LocalHolder(new Payload(5));
  assert(l.doubled() === 10, "local subclass of an imported class-typed ctor");
  assert(l.tag === "local", "local subclass's super() args");

  const f = new Forwarder(new Payload(9));
  assert(f.total() === 9, "implicit ctor forwarding a class-typed param");

  const pp = new PropHolder(new BigPayload(11), null);
  assert(pp.p.n === 110, "class-typed parameter property across the boundary");
  assert(pp.maybe === null, "nullable class-typed parameter property");

  assert(new Early(new Late()).v === 8, "ctor param typed by a later-declared class");

  assert(new MaybeHolder(new Payload(2)).n === 2, "nullable class-typed ctor param");
  assert(new MaybeHolder(null).n === -1, "null into a nullable class-typed ctor param");

  assert(new EitherHolder(new Payload(6)).n === 6, "non-nullable class union ctor param");
  assert(new EitherHolder(new Marker()).n === 100, "the other arm of the class union");

  const b = new Box<number>(21, new Payload(1));
  assert(b.value === 21, "generic ctor: erased `T` beside a class-typed param");
  assert(b.tagged() === 1, "generic ctor's class-typed param");

  const pb = new PayloadBox(new Payload(2), new Payload(3));
  assert(pb.value.n === 2, "generic parent instantiated at a class type");
  assert(pb.tagged() === 3, "implicit ctor forwarding through a generic parent");

  const lb = new LocalBox("s", new Payload(4));
  assert(lb.value === "s", "local subclass of an imported generic class-typed ctor");
  assert(lb.tagged() === 4, "class-typed param through a local implicit ctor");
}
