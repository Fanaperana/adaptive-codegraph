// TYPESCRIPT EDGE CASES — comprehensive test for tricky patterns

// === 1. Complex generic types ===
export type DeepPartial<T> = {
  [P in keyof T]?: T[P] extends object ? DeepPartial<T[P]> : T[P];
};

export type Awaited<T> = T extends Promise<infer U> ? Awaited<U> : T;

export type EventMap = {
  click: MouseEvent;
  keydown: KeyboardEvent;
  scroll: Event;
};

// === 2. Intersection and union types ===
export type Serializable = string | number | boolean | null;
export type WithTimestamp<T> = T & { createdAt: Date; updatedAt: Date };

// === 3. Template literal types ===
export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE";
export type ApiRoute = `/api/${string}`;

// === 4. Enum with computed values ===
export enum BitFlags {
  None = 0,
  Read = 1 << 0,
  Write = 1 << 1,
  Execute = 1 << 2,
  All = Read | Write | Execute,
}

// === 5. Const enum (inlined at compile time) ===
export const enum Direction {
  Up = "UP",
  Down = "DOWN",
  Left = "LEFT",
  Right = "RIGHT",
}

// === 6. Abstract class with abstract methods ===
export abstract class BaseHandler<TReq, TRes> {
  abstract handle(req: TReq): Promise<TRes>;
  abstract validate(req: TReq): boolean;

  protected log(message: string): void {
    console.log(`[${this.constructor.name}] ${message}`);
  }
}

// === 7. Class with private fields, getters/setters, static members ===
export class StateManager<T extends object> {
  #state: T;
  #listeners: Set<(state: T) => void> = new Set();
  static instanceCount = 0;

  constructor(initial: T) {
    this.#state = initial;
    StateManager.instanceCount++;
  }

  get current(): T {
    return { ...this.#state };
  }

  set state(next: T) {
    this.#state = next;
    this.notify();
  }

  static reset(): void {
    StateManager.instanceCount = 0;
  }

  private notify(): void {
    this.#listeners.forEach((fn) => fn(this.#state));
  }

  subscribe(listener: (state: T) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }
}

// === 8. Interface with call/index/construct signatures ===
export interface StringMap {
  [key: string]: string;
}

export interface Callable {
  (input: string): number;
  readonly name: string;
}

export interface Constructable<T> {
  new (args: unknown[]): T;
}

// === 9. Interface extending multiple interfaces ===
export interface Timestamped {
  createdAt: Date;
  updatedAt: Date;
}

export interface Identifiable {
  id: string;
}

export interface Entity extends Timestamped, Identifiable {
  version: number;
}

// === 10. Function overloads ===
export function parse(input: string): number;
export function parse(input: string, radix: number): number;
export function parse(input: string, radix?: number): number {
  return parseInt(input, radix ?? 10);
}

// === 11. Const assertions and readonly arrays ===
export const ROLES = ["admin", "editor", "viewer"] as const;
export type Role = (typeof ROLES)[number];

export const CONFIG = {
  maxRetries: 3,
  timeout: 5000,
  endpoints: {
    users: "/api/users",
    posts: "/api/posts",
  },
} as const;

// === 12. Decorator patterns (experimental) ===
function sealed(constructor: Function) {
  Object.seal(constructor);
  Object.seal(constructor.prototype);
}

function log(target: any, key: string, descriptor: PropertyDescriptor) {
  const original = descriptor.value;
  descriptor.value = function (...args: any[]) {
    console.log(`Calling ${key} with`, args);
    return original.apply(this, args);
  };
}

// === 13. Generic function with constraints ===
export function merge<T extends object, U extends object>(a: T, b: U): T & U {
  return { ...a, ...b };
}

export async function* asyncGenerator(n: number): AsyncGenerator<number> {
  for (let i = 0; i < n; i++) {
    yield i;
    await new Promise((r) => setTimeout(r, 100));
  }
}

// === 14. Namespace (ambient module) ===
export namespace Validators {
  export function isEmail(s: string): boolean {
    return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s);
  }

  export function isUUID(s: string): boolean {
    return /^[0-9a-f]{8}-/.test(s);
  }

  export const MAX_LENGTH = 255;
}

// === 15. Mapped type with conditional ===
export type Mutable<T> = {
  -readonly [P in keyof T]: T[P];
};

export type NonNullableFields<T> = {
  [P in keyof T]: NonNullable<T[P]>;
};

// === 16. Discriminated union ===
export type Shape =
  | { kind: "circle"; radius: number }
  | { kind: "rect"; width: number; height: number }
  | { kind: "triangle"; base: number; height: number };

export function area(shape: Shape): number {
  switch (shape.kind) {
    case "circle":
      return Math.PI * shape.radius ** 2;
    case "rect":
      return shape.width * shape.height;
    case "triangle":
      return 0.5 * shape.base * shape.height;
  }
}

// === 17. Re-exports and barrel pattern ===
export { StateManager as SM } from "./edge_cases";
