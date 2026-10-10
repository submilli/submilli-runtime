import { Zu } from "@test/ubase";

export class Api {
  constructor(readonly label: string) {}
  static readonly T: Zu = new Zu(7);
  static make(): Zu {
    return Api.T;
  }
}
