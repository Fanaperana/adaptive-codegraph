// JAVASCRIPT EDGE CASES — patterns that stress the parser

// === 1. IIFE (Immediately Invoked Function Expression) ===
const moduleState = (function () {
  let counter = 0;
  return {
    increment: () => ++counter,
    decrement: () => --counter,
    getCount: () => counter,
  };
})();

// === 2. Generator function ===
function* fibonacci() {
  let a = 0, b = 1;
  while (true) {
    yield a;
    [a, b] = [b, a + b];
  }
}

// === 3. Async generator ===
async function* fetchPages(baseUrl) {
  let page = 1;
  while (true) {
    const res = await fetch(`${baseUrl}?page=${page}`);
    const data = await res.json();
    if (data.length === 0) return;
    yield data;
    page++;
  }
}

// === 4. Computed property names ===
const ACTIONS = {
  ["INCREMENT"]: (state) => state + 1,
  ["DECREMENT"]: (state) => state - 1,
  ["RESET"]: () => 0,
};

// === 5. Symbol-keyed methods ===
const serialize = Symbol("serialize");
const deserialize = Symbol("deserialize");

class CustomMap {
  #data = new Map();

  set(key, value) {
    this.#data.set(key, value);
    return this;
  }

  get(key) {
    return this.#data.get(key);
  }

  [serialize]() {
    return JSON.stringify([...this.#data]);
  }

  [deserialize](str) {
    const entries = JSON.parse(str);
    this.#data = new Map(entries);
  }

  get size() {
    return this.#data.size;
  }

  [Symbol.iterator]() {
    return this.#data[Symbol.iterator]();
  }
}

// === 6. Prototype-based inheritance (legacy pattern) ===
function Animal(name, sound) {
  this.name = name;
  this.sound = sound;
}

Animal.prototype.speak = function () {
  return `${this.name} says ${this.sound}`;
};

function Dog(name) {
  Animal.call(this, name, "woof");
}

Dog.prototype = Object.create(Animal.prototype);
Dog.prototype.constructor = Dog;
Dog.prototype.fetch = function (item) {
  return `${this.name} fetches ${item}`;
};

// === 7. Object.defineProperty ===
const config = {};
Object.defineProperty(config, "apiKey", {
  get() { return process.env.API_KEY; },
  enumerable: false,
  configurable: false,
});

// === 8. Destructured exports ===
const add = (a, b) => a + b;
const subtract = (a, b) => a - b;
const multiply = (a, b) => a * b;
const divide = (a, b) => {
  if (b === 0) throw new Error("Division by zero");
  return a / b;
};
export { add, subtract, multiply, divide };

// === 9. Default export with expression ===
export default class extends Error {
  constructor(message, code) {
    super(message);
    this.code = code;
    this.name = "AppError";
  }

  toJSON() {
    return { name: this.name, message: this.message, code: this.code };
  }
}

// === 10. Tagged template literal function ===
function sql(strings, ...values) {
  return {
    text: strings.join("?"),
    values,
  };
}

// === 11. Proxy-based reactive object ===
function reactive(target) {
  return new Proxy(target, {
    get(obj, prop) {
      console.log(`Reading ${String(prop)}`);
      return Reflect.get(obj, prop);
    },
    set(obj, prop, value) {
      console.log(`Writing ${String(prop)} = ${value}`);
      return Reflect.set(obj, prop, value);
    },
  });
}

// === 12. WeakRef and FinalizationRegistry ===
const registry = new FinalizationRegistry((key) => {
  console.log(`Cleaned up: ${key}`);
});

class Cache {
  #refs = new Map();

  set(key, value) {
    const ref = new WeakRef(value);
    this.#refs.set(key, ref);
    registry.register(value, key);
  }

  get(key) {
    const ref = this.#refs.get(key);
    return ref?.deref();
  }
}

// === 13. Promise combinators ===
async function resilientFetch(urls) {
  const results = await Promise.allSettled(
    urls.map((url) => fetch(url).then((r) => r.json()))
  );
  return results
    .filter((r) => r.status === "fulfilled")
    .map((r) => r.value);
}

// === 14. Nested arrow function callbacks ===
const pipeline = (initialValue) => ({
  pipe: (fn) => pipeline(fn(initialValue)),
  value: () => initialValue,
});

// === 15. Dynamic import ===
async function loadModule(name) {
  const mod = await import(`./modules/${name}.js`);
  return mod.default;
}

// === 16. for-await-of consumer ===
async function consumeStream(stream) {
  const chunks = [];
  for await (const chunk of stream) {
    chunks.push(chunk);
  }
  return Buffer.concat(chunks);
}

// === 17. Nullish coalescing and optional chaining ===
const getNestedValue = (obj, path) =>
  path.split(".").reduce((acc, key) => acc?.[key] ?? null, obj);

export {
  moduleState,
  fibonacci,
  fetchPages,
  CustomMap,
  Animal,
  Dog,
  Cache,
  resilientFetch,
  pipeline,
  loadModule,
  consumeStream,
  reactive,
  sql,
  getNestedValue,
};
