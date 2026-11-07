import { checksum } from "./util";

export class Store { // @def Store
  private items: string[] = [];

  save(item: string): void { // @def Store::save
    const sum = checksum(item);
    this.items.push(`${item}:${sum}`);
    this.flush();
  }

  size(): number { // @def Store::size
    return this.items.length;
  }

  private flush(): void { // @def Store::flush
    console.log("flush", this.items.map((x) => x.length));
  }
}
