export function checksum(text: string): number { // @def util::checksum
  return text.split("").reduce((a, c) => a + c.charCodeAt(0), 0);
}

export function helper(): number { // @def util::helper
  return checksum("h");
}

export const format = (n: number): string => n.toFixed(2); // @def util::format
