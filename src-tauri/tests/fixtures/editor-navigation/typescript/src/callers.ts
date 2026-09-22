import { target } from "./target";

export function callerA(): void {
  target();
}

export function callerB(): void {
  target();
}
